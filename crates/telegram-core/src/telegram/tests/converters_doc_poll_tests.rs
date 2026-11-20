//! Converter tests: document and poll metadata extraction.
use super::av_tests::{audio_doc, video_doc};
use super::*;
use grammers_client::media::{Document, Media, Poll};
use grammers_client::tl;

fn plain_doc(file_name: Option<&str>, size: i64, mime: &str) -> Media {
    let attributes = file_name
        .map(|name| {
            vec![tl::enums::DocumentAttribute::Filename(
                tl::types::DocumentAttributeFilename {
                    file_name: name.to_string(),
                },
            )]
        })
        .unwrap_or_default();
    Media::Document(Document::from_raw_media(tl::types::MessageMediaDocument {
        nopremium: false,
        spoiler: false,
        video: false,
        round: false,
        voice: false,
        document: Some(tl::enums::Document::Document(tl::types::Document {
            id: 1,
            access_hash: 0,
            file_reference: Vec::new(),
            date: 0,
            mime_type: mime.to_string(),
            size,
            thumbs: None,
            video_thumbs: None,
            dc_id: 0,
            attributes,
        })),
        alt_documents: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    }))
}

#[test]
fn document_info_reads_filename_size_and_mime() {
    let media = plain_doc(Some("Как мы строим RAG.pdf"), 2_411_008, "application/pdf");

    let info = extract_document_info(&media).expect("document info present");

    assert_eq!(info.file_name.as_deref(), Some("Как мы строим RAG.pdf"));
    assert_eq!(info.file_size_bytes, 2_411_008);
    assert_eq!(info.mime_type.as_deref(), Some("application/pdf"));
}

#[test]
fn document_info_without_filename_attribute_omits_the_name() {
    let media = plain_doc(None, 512, "application/zip");

    let info = extract_document_info(&media).expect("document info present");

    assert_eq!(info.file_name, None);
    assert_eq!(info.file_size_bytes, 512);
}

#[test]
fn document_info_is_none_for_video_media() {
    let media = video_doc(false, 30.0, 1920, 1080, 5_000_000, "video/mp4", true);

    assert!(extract_document_info(&media).is_none());
}

#[test]
fn document_info_is_none_for_audio_media() {
    let media = audio_doc(false, 184, 7_340_032, "audio/mpeg", None, None);

    assert!(extract_document_info(&media).is_none());
}

#[test]
fn document_info_absent_from_json_when_media_is_not_a_document() {
    let msg = crate::test_helpers::create_test_message(1, "текст", 100);

    let json = serde_json::to_value(&msg).expect("serializes");

    assert!(json.get("document_info").is_none());
}

fn poll_media(
    question: &str,
    answers: &[&str],
    voters: Option<&[i32]>,
    total_voters: Option<i32>,
    closed: bool,
    multiple_choice: bool,
    quiz: bool,
) -> Media {
    let text = |s: &str| {
        tl::enums::TextWithEntities::Entities(tl::types::TextWithEntities {
            text: s.to_string(),
            entities: Vec::new(),
        })
    };
    // The `option` bytes are the key linking an answer to its vote count.
    let raw_answers = answers
        .iter()
        .enumerate()
        .map(|(i, a)| {
            tl::enums::PollAnswer::Answer(tl::types::PollAnswer {
                text: text(a),
                option: vec![i as u8],
                media: None,
                added_by: None,
                date: None,
            })
        })
        .collect();
    let results = voters.map(|counts| {
        counts
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                tl::enums::PollAnswerVoters::Voters(tl::types::PollAnswerVoters {
                    chosen: false,
                    correct: false,
                    option: vec![i as u8],
                    voters: Some(v),
                    recent_voters: None,
                })
            })
            .collect()
    });
    Media::Poll(Poll::from_raw_media(tl::types::MessageMediaPoll {
        poll: tl::enums::Poll::Poll(tl::types::Poll {
            id: 1,
            closed,
            public_voters: false,
            multiple_choice,
            quiz,
            open_answers: false,
            revoting_disabled: false,
            shuffle_answers: false,
            hide_results_until_close: false,
            creator: false,
            subscribers_only: false,
            question: text(question),
            answers: raw_answers,
            close_period: None,
            close_date: None,
            countries_iso2: None,
            hash: 0,
        }),
        results: tl::enums::PollResults::Results(Box::new(tl::types::PollResults {
            min: false,
            has_unread_votes: false,
            can_view_stats: false,
            results,
            total_voters,
            recent_voters: None,
            solution: None,
            solution_entities: None,
            solution_media: None,
        })),
        attached_media: None,
    }))
}

#[test]
fn poll_info_reads_question_options_and_per_option_voters() {
    let media = poll_media(
        "Какой стек выбрать?",
        &["Rust", "Go"],
        Some(&[287, 125]),
        Some(412),
        true,
        false,
        false,
    );

    let info = extract_poll_info(&media).expect("poll info present");

    assert_eq!(info.question, "Какой стек выбрать?");
    assert_eq!(info.options.len(), 2);
    assert_eq!(info.options[0].text, "Rust");
    assert_eq!(info.options[0].voters, Some(287));
    assert_eq!(info.options[1].text, "Go");
    assert_eq!(info.options[1].voters, Some(125));
    assert_eq!(info.total_voters, Some(412));
    assert!(info.closed);
    assert!(!info.multiple_choice);
    assert!(!info.quiz);
}

#[test]
fn poll_info_without_results_keeps_options_and_omits_voters() {
    let media = poll_media(
        "Придёте на митап?",
        &["Да", "Нет"],
        None,
        None,
        false,
        false,
        false,
    );

    let info = extract_poll_info(&media).expect("poll info present");

    assert_eq!(info.options.len(), 2);
    assert_eq!(info.options[0].voters, None);
    assert_eq!(info.options[1].voters, None);
    assert_eq!(info.total_voters, None);
    assert!(!info.closed);
}

#[test]
fn poll_info_matches_voters_to_options_by_key_not_position() {
    // Results arrive in a different order than the answers: option b'\x01'
    // (Go) first. Matching by the `option` bytes must still be correct.
    let mut media = poll_media(
        "Какой стек выбрать?",
        &["Rust", "Go"],
        Some(&[287, 125]),
        Some(412),
        false,
        false,
        false,
    );
    if let Media::Poll(ref mut poll) = media
        && let Some(results) = poll.raw_results.results.as_mut()
    {
        results.reverse();
    }

    let info = extract_poll_info(&media).expect("poll info present");

    assert_eq!(info.options[0].text, "Rust");
    assert_eq!(info.options[0].voters, Some(287));
    assert_eq!(info.options[1].text, "Go");
    assert_eq!(info.options[1].voters, Some(125));
}

#[test]
fn poll_info_flags_a_quiz_and_multiple_choice() {
    let media = poll_media("2+2?", &["4", "5"], None, None, false, true, true);

    let info = extract_poll_info(&media).expect("poll info present");

    assert!(info.quiz);
    assert!(info.multiple_choice);
}

#[test]
fn poll_info_is_none_for_non_poll_media() {
    let media = plain_doc(Some("slides.pdf"), 1024, "application/pdf");

    assert!(extract_poll_info(&media).is_none());
}

#[test]
fn poll_option_omits_absent_voters_from_json() {
    let media = poll_media("Придёте?", &["Да"], None, None, false, false, false);
    let info = extract_poll_info(&media).expect("poll info present");

    let json = serde_json::to_value(&info).expect("serializes");

    assert!(json["options"][0].get("voters").is_none());
    assert!(json.get("total_voters").is_none());
}

#[test]
fn poll_info_omits_voters_for_an_option_whose_count_is_undisclosed() {
    // `PollAnswerVoters.voters` carries its own disclosure flag, independent
    // of whether `results` is populated at all: Telegram's partial-
    // disclosure case reveals which option is chosen/correct while still
    // withholding that option's vote count. A `None` there must degrade to
    // an absent `voters` on the option — never a fabricated `0`.
    let text = |s: &str| {
        tl::enums::TextWithEntities::Entities(tl::types::TextWithEntities {
            text: s.to_string(),
            entities: Vec::new(),
        })
    };
    let raw_answers = ["Rust", "Go"]
        .into_iter()
        .enumerate()
        .map(|(i, a)| {
            tl::enums::PollAnswer::Answer(tl::types::PollAnswer {
                text: text(a),
                option: vec![i as u8],
                media: None,
                added_by: None,
                date: None,
            })
        })
        .collect();
    let results = vec![
        tl::enums::PollAnswerVoters::Voters(tl::types::PollAnswerVoters {
            chosen: false,
            correct: false,
            option: vec![0u8],
            voters: Some(287),
            recent_voters: None,
        }),
        // Same wire shape as above, but the count itself is undisclosed.
        tl::enums::PollAnswerVoters::Voters(tl::types::PollAnswerVoters {
            chosen: false,
            correct: false,
            option: vec![1u8],
            voters: None,
            recent_voters: None,
        }),
    ];
    let media = Media::Poll(Poll::from_raw_media(tl::types::MessageMediaPoll {
        poll: tl::enums::Poll::Poll(tl::types::Poll {
            id: 1,
            closed: false,
            public_voters: false,
            multiple_choice: false,
            quiz: false,
            open_answers: false,
            revoting_disabled: false,
            shuffle_answers: false,
            hide_results_until_close: false,
            creator: false,
            subscribers_only: false,
            question: text("Какой стек выбрать?"),
            answers: raw_answers,
            close_period: None,
            close_date: None,
            countries_iso2: None,
            hash: 0,
        }),
        results: tl::enums::PollResults::Results(Box::new(tl::types::PollResults {
            min: false,
            has_unread_votes: false,
            can_view_stats: false,
            results: Some(results),
            total_voters: Some(412),
            recent_voters: None,
            solution: None,
            solution_entities: None,
            solution_media: None,
        })),
        attached_media: None,
    }));

    let info = extract_poll_info(&media).expect("poll info present");

    assert_eq!(info.options[0].text, "Rust");
    assert_eq!(info.options[0].voters, Some(287));
    assert_eq!(info.options[1].text, "Go");
    assert_eq!(info.options[1].voters, None);
    assert_eq!(info.total_voters, Some(412));
}
