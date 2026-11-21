use super::*;

const OWNER: &str = "fixture owner";
const BASIC: &str = "chat:42";
const SUPERGROUP: &str = "supergroup:42";
const GENERATION: &str = "first generation";
const DEFAULT_RETENTION: Retention = Retention {
    seconds: 86_400,
    bytes: 64 * 1024 * 1024,
};

fn edit(source: &str, body: &str) -> PendingChange {
    PendingChange {
        source: Some(source.into()),
        change: Change::Edited {
            message_id: "104".into(),
            edited_at: "2026-11-21T10:04:01Z".into(),
            text: body.into(),
        },
    }
}

fn at(seconds: i64) -> UtcTimestamp {
    UtcTimestamp::from_unix_seconds(seconds).expect("test observation time")
}

async fn open(root: &Path, limits: Retention) -> GroupJournal {
    GroupJournal::open(root, limits)
        .await
        .expect("open private journal")
}

async fn page(journal: &GroupJournal, group: &str, after: Option<&str>, limit: usize) -> Page {
    let PageResult::Page(page) = journal
        .page(GENERATION, OWNER, group, after, limit)
        .await
        .expect("read journal page")
    else {
        panic!("expected a journal page");
    };
    page
}

#[tokio::test]
async fn restart_replays_same_second_edits_and_keeps_group_types_separate() {
    let root = tempfile::tempdir().expect("journal directory");
    let journal = open(root.path(), DEFAULT_RETENTION).await;
    for group in [BASIC, SUPERGROUP] {
        assert!(
            journal
                .enable(GENERATION, group)
                .await
                .expect("enable group")
        );
    }
    let initial = page(&journal, BASIC, None, 100).await;
    assert_eq!(initial.base_seq, "0");
    assert!(initial.entries.is_empty());
    assert!(!initial.has_more);
    for group in [BASIC, SUPERGROUP] {
        journal
            .append(
                GENERATION,
                group,
                vec![edit("42", "first edit"), edit("43", "second edit")],
            )
            .await
            .expect("commit both edits");
    }
    let first_page = page(&journal, BASIC, Some(&initial.base_cursor), 1).await;
    assert!(first_page.has_more);
    assert_eq!(first_page.entries[0].seq, "1");
    let last_page = page(&journal, BASIC, Some(&first_page.entries[0].cursor), 1).await;
    assert_eq!(last_page.base_seq, "1");
    assert_eq!(last_page.entries[0].seq, "2");
    assert!(!last_page.has_more);
    drop(journal);

    let journal = open(root.path(), DEFAULT_RETENTION).await;
    assert!(
        !journal
            .enable(GENERATION, BASIC)
            .await
            .expect("existing group")
    );
    journal
        .append(GENERATION, BASIC, vec![edit("42", "duplicate delivery")])
        .await
        .expect("duplicate is safe");
    let replay = page(&journal, BASIC, Some(&initial.base_cursor), 1).await;
    assert_eq!(
        serde_json::to_value(&first_page).expect("original response"),
        serde_json::to_value(&replay).expect("retry response")
    );
    let empty = page(&journal, BASIC, Some(&last_page.entries[0].cursor), 100).await;
    assert_eq!(empty.base_seq, "2");
    assert_eq!(empty.base_cursor, last_page.entries[0].cursor);
    assert!(empty.entries.is_empty());
    assert!(!empty.has_more);
    assert_ne!(
        page(&journal, SUPERGROUP, None, 100).await.stream_id,
        initial.stream_id
    );
    assert!(matches!(
        journal
            .page(GENERATION, OWNER, SUPERGROUP, Some(&initial.base_cursor), 1)
            .await
            .expect("typed cursor validation"),
        PageResult::InvalidCursor
    ));
}

#[tokio::test]
async fn expiration_and_rebinding_return_the_earliest_boundary_to_the_same_owner() {
    let root = tempfile::tempdir().expect("journal directory");
    let limits = Retention {
        seconds: 100,
        ..DEFAULT_RETENTION
    };
    let journal = open(root.path(), limits).await;
    assert!(GroupJournal::exists(root.path()));
    journal
        .enable(GENERATION, BASIC)
        .await
        .expect("enable group");
    let initial = page(&journal, BASIC, None, 100).await;
    let now = UtcTimestamp::now().unix_seconds();
    journal
        .append_at(GENERATION, BASIC, vec![edit("7", "old")], at(now - 101))
        .await
        .expect("seed expired edit");
    journal
        .append(GENERATION, BASIC, vec![edit("8", "current")])
        .await
        .expect("seed current edit");
    journal.reap().await.expect("run retention maintenance");
    drop(journal);

    let journal = open(root.path(), limits).await;
    let PageResult::Expired(expired) = journal
        .page(GENERATION, OWNER, BASIC, Some(&initial.base_cursor), 100)
        .await
        .expect("expired cursor response")
    else {
        panic!("expected expired cursor");
    };
    assert_eq!(expired.stream_id, initial.stream_id);
    assert_eq!(expired.base_seq, "1");
    assert_eq!(
        page(&journal, BASIC, Some(&expired.base_cursor), 100)
            .await
            .entries[0]
            .seq,
        "2"
    );

    journal
        .retire(GENERATION)
        .await
        .expect("retire old account link");
    assert!(matches!(
        journal.enable(GENERATION, BASIC).await,
        Err(StorageError::Missing)
    ));
    journal.enable("new", BASIC).await.expect("relink group");
    assert_eq!(
        journal
            .generations_after("")
            .await
            .expect("active journal generations"),
        ["new"]
    );
    assert!(
        journal
            .generations_after("new")
            .await
            .expect("end of generation scan")
            .is_empty()
    );
    let PageResult::Expired(relinked) = journal
        .page("new", OWNER, BASIC, Some(&initial.base_cursor), 100)
        .await
        .expect("old generation response")
    else {
        panic!("expected expired cursor from old generation");
    };
    assert_ne!(relinked.stream_id, initial.stream_id);
    assert_eq!(relinked.base_seq, "0");
    assert!(matches!(
        journal
            .page("new", "other owner", BASIC, Some(&initial.base_cursor), 100)
            .await
            .expect("owner-bound cursor validation"),
        PageResult::InvalidCursor
    ));

    drop(journal);
    std::fs::remove_file(root.path().join(JOURNAL_DB)).expect("recreate lost journal");
    let journal = open(root.path(), limits).await;
    journal.enable("new", BASIC).await.expect("recreate group");
    let PageResult::Expired(recreated) = journal
        .page("new", OWNER, BASIC, Some(&relinked.base_cursor), 100)
        .await
        .expect("recreated journal response")
    else {
        panic!("expected expired cursor from lost journal");
    };
    assert_ne!(recreated.stream_id, relinked.stream_id);
    assert_eq!(recreated.base_seq, "0");
}

#[tokio::test]
async fn byte_retention_keeps_whole_entries_and_independent_deletion_attribution() {
    let root = tempfile::tempdir().expect("journal directory");
    let journal = open(
        root.path(),
        Retention {
            bytes: 20_000,
            ..DEFAULT_RETENTION
        },
    )
    .await;
    journal
        .enable(GENERATION, BASIC)
        .await
        .expect("enable group");
    let body = "\u{0001}".repeat(2_000);
    journal
        .append(
            GENERATION,
            BASIC,
            vec![edit("10", &body), edit("11", &body), edit("12", &body)],
        )
        .await
        .expect("commit page entries");
    let retained = page(&journal, BASIC, None, 100).await;
    assert_eq!(retained.base_seq, "2");
    assert_eq!(retained.entries.len(), 1);
    assert_eq!(retained.entries[0].seq, "3");
    assert!(
        journal
            .known_source(GENERATION, BASIC, "10")
            .await
            .expect("lookup deduplication key")
    );
    assert!(
        journal
            .known_message(GENERATION, BASIC, "104")
            .await
            .expect("lookup text index")
    );
    journal
        .append(
            GENERATION,
            BASIC,
            vec![PendingChange {
                source: Some("13".into()),
                change: Change::Deleted {
                    message_id: "104".into(),
                    deleted_at: String::new(),
                },
            }],
        )
        .await
        .expect("commit deletion");
    assert!(
        !journal
            .known_message(GENERATION, BASIC, "104")
            .await
            .expect("lookup deletion")
    );
    let deletion = page(&journal, BASIC, Some(&retained.entries[0].cursor), 100).await;
    assert!(matches!(
        &deletion.entries[0].change,
        Change::Deleted { deleted_at, .. } if deleted_at == &deletion.entries[0].observed_at
    ));
}

#[tokio::test]
async fn page_stops_at_the_encoded_byte_cap_between_escaped_entries() {
    let root = tempfile::tempdir().expect("journal directory");
    let journal = open(root.path(), DEFAULT_RETENTION).await;
    journal
        .enable(GENERATION, BASIC)
        .await
        .expect("enable group");
    let body = "\u{0001}".repeat(4_096);
    journal
        .append(
            GENERATION,
            BASIC,
            (0..20)
                .map(|source| edit(&source.to_string(), &body))
                .collect(),
        )
        .await
        .expect("commit page entries");
    let first = page(&journal, BASIC, None, 100).await;
    assert!(first.has_more);
    assert!(serde_json::to_vec(&first).expect("page encoding").len() <= MAX_PAGE_BYTES);
    let last_entry = first.entries.last().expect("whole entries");
    let rest = page(&journal, BASIC, Some(&last_entry.cursor), 100).await;
    assert_eq!(first.entries.len() + rest.entries.len(), 20);
    assert_eq!(rest.entries[0].seq, (first.entries.len() + 1).to_string());
    assert!(!rest.has_more);
}

#[tokio::test]
async fn failed_batch_advances_neither_boundary_nor_deduplication_index() {
    let root = tempfile::tempdir().expect("journal directory");
    let journal = open(root.path(), DEFAULT_RETENTION).await;
    journal
        .enable(GENERATION, BASIC)
        .await
        .expect("enable group");
    let initial = page(&journal, BASIC, None, 100).await;
    journal
        .db
        .lock()
        .await
        .execute_batch(
            "CREATE TRIGGER interrupt_second_insert BEFORE INSERT ON group_entries
             WHEN new.seq = 2 BEGIN SELECT RAISE(ABORT, 'test interruption'); END;",
        )
        .await
        .expect("simulate an interrupted journal write");

    let changes = || vec![edit("42", "first edit"), edit("43", "second edit")];
    assert!(matches!(
        journal.append(GENERATION, BASIC, changes()).await,
        Err(StorageError::Unavailable)
    ));
    let interrupted = page(&journal, BASIC, Some(&initial.base_cursor), 100).await;
    assert_eq!(interrupted.base_seq, "0");
    assert!(interrupted.entries.is_empty());
    assert!(
        !journal
            .known_source(GENERATION, BASIC, "42")
            .await
            .expect("lookup source")
    );
    assert!(
        !journal
            .known_message(GENERATION, BASIC, "104")
            .await
            .expect("lookup text ID")
    );

    journal
        .db
        .lock()
        .await
        .execute_batch("DROP TRIGGER interrupt_second_insert;")
        .await
        .expect("restore writable journal");
    journal
        .append(GENERATION, BASIC, changes())
        .await
        .expect("retry interrupted batch");
    let retried = page(&journal, BASIC, Some(&initial.base_cursor), 100).await;
    assert_eq!(
        retried
            .entries
            .iter()
            .map(|entry| entry.seq.as_str())
            .collect::<Vec<_>>(),
        ["1", "2"]
    );
}
