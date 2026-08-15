//! Album grouping: post-level limit counting and album collapsing (B5+A2).

use crate::telegram::types::{AlbumInfo, Message};
use std::collections::{HashMap, HashSet};

/// Channel-scoped album key: `grouped_id` alone can collide across channels
/// in global-search results (e.g. a forwarded album), so grouping always
/// pairs it with the channel id.
pub(crate) fn album_key(msg: &Message) -> Option<(i64, i64)> {
    msg.grouped_id.map(|gid| (msg.channel_id.get(), gid))
}

/// Counts posts (albums count once) while a fetch loop admits messages.
#[derive(Debug, Default)]
pub(crate) struct PostCounter {
    seen_groups: HashSet<(i64, i64)>,
    posts: usize,
    overflowed: bool,
}

impl PostCounter {
    /// Admit a message into a window capped at `limit` posts. Returns `false`
    /// when the message would START a post beyond the limit — the caller
    /// stops fetching. Siblings of an already-admitted album are always
    /// admitted, so an album is never cut at the limit boundary (A2).
    pub(crate) fn admit(&mut self, group_key: Option<(i64, i64)>, limit: usize) -> bool {
        if let Some(key) = group_key
            && self.seen_groups.contains(&key)
        {
            return true;
        }
        if self.posts >= limit {
            self.overflowed = true;
            return false;
        }
        self.posts += 1;
        if let Some(key) = group_key {
            self.seen_groups.insert(key);
        }
        true
    }

    /// True iff `admit` ever refused a message — i.e. a qualifying post
    /// existed beyond the limit, so the caller's result page is not the
    /// end of the window (work-order A8 `has_more`). Fetch loops capture
    /// the refusal inline at the `admit` call site, so production code
    /// never reads this back; test-only accessor.
    #[cfg(test)]
    pub(crate) fn overflowed(&self) -> bool {
        self.overflowed
    }
}

/// Accumulates one result page inside a fetch loop, owning the post-level
/// limit decision the three fetch loops used to hand-inline (audit S3.2):
/// with `collapse_albums`, a message that would START a post beyond `limit`
/// is refused (trailing siblings of admitted albums pass — A2); without it,
/// the limit-th overflow message is refused instead of pushed blind, proving
/// a qualifying message exists beyond the page (A8). A refusal latches
/// `has_more`.
#[derive(Debug)]
pub(crate) struct PageAccumulator {
    messages: Vec<Message>,
    counter: PostCounter,
    collapse: bool,
    limit: usize,
    has_more: bool,
}

impl PageAccumulator {
    pub(crate) fn new(collapse_albums: bool, limit: usize) -> Self {
        Self {
            messages: Vec::new(),
            counter: PostCounter::default(),
            collapse: collapse_albums,
            limit,
            has_more: false,
        }
    }

    /// Admit `message` into the page. Returns `false` when the page is full —
    /// the caller stops fetching.
    pub(crate) fn push(&mut self, message: Message) -> bool {
        let admitted = if self.collapse {
            self.counter.admit(album_key(&message), self.limit)
        } else {
            self.messages.len() < self.limit
        };
        if !admitted {
            self.has_more = true;
            return false;
        }
        self.messages.push(message);
        true
    }

    pub(crate) fn has_more(&self) -> bool {
        self.has_more
    }

    /// Messages admitted so far (pre-collapse) — used by progress logging.
    pub(crate) fn len(&self) -> usize {
        self.messages.len()
    }

    /// Finish the page: collapse album siblings when enabled.
    pub(crate) fn into_messages(self) -> Vec<Message> {
        if self.collapse {
            collapse_albums(self.messages)
        } else {
            self.messages
        }
    }
}

/// Collapse album siblings (same `grouped_id`) into one post-level `Message`
/// (B5). Order-preserving on each group's first occurrence. The representative
/// is the lowest-id sibling (stable referencing); `text` comes from whichever
/// sibling carries it. A group with a single member in the window stays plain.
pub(crate) fn collapse_albums(messages: Vec<Message>) -> Vec<Message> {
    enum Slot {
        Single(Box<Message>),
        Group((i64, i64)),
    }

    let mut slots = Vec::new();
    let mut buckets: HashMap<(i64, i64), Vec<Message>> = HashMap::new();
    for msg in messages {
        match album_key(&msg) {
            Some(key) => {
                let bucket = buckets.entry(key).or_default();
                if bucket.is_empty() {
                    slots.push(Slot::Group(key));
                }
                bucket.push(msg);
            }
            None => slots.push(Slot::Single(Box::new(msg))),
        }
    }

    slots
        .into_iter()
        .filter_map(|slot| match slot {
            Slot::Single(msg) => Some(*msg),
            Slot::Group(key) => {
                let mut siblings = buckets.remove(&key)?;
                siblings.sort_by_key(|m| m.id.get());
                if siblings.len() == 1 {
                    return siblings.pop();
                }
                let text = siblings
                    .iter()
                    .find(|m| !m.text.is_empty())
                    .map(|m| m.text.clone())
                    .unwrap_or_default();
                let album = AlbumInfo {
                    media_count: siblings.len() as u32,
                    media_types: siblings.iter().map(|m| m.media_type).collect(),
                    message_ids: siblings.iter().map(|m| m.id).collect(),
                };
                let mut post = siblings.swap_remove(0); // lowest id after the sort
                post.text = text;
                post.album = Some(album);
                Some(post)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::types::MediaType;
    use crate::test_helpers::create_test_message;

    fn album_member(id: i64, gid: i64, text: &str) -> crate::telegram::types::Message {
        let mut m = create_test_message(id, text, 100);
        m.grouped_id = Some(gid);
        m.has_media = true;
        m.media_type = MediaType::Photo;
        m
    }

    #[test]
    fn audit_fixture_collapses_to_three_posts() {
        // Work-order fixture: 8 siblings (610047–610054) + 2 singles → 3 posts.
        let mut messages: Vec<_> = (610047..=610054)
            .map(|id| album_member(id, 13950, if id == 610047 { "album caption" } else { "" }))
            .collect();
        messages.push(create_test_message(610119, "single one", 100));
        messages.push(create_test_message(610121, "single two", 100));

        let collapsed = collapse_albums(messages);

        assert_eq!(collapsed.len(), 3);
        let post = &collapsed[0];
        assert_eq!(
            post.id.get(),
            610047,
            "representative is the lowest sibling id"
        );
        assert_eq!(post.text, "album caption", "text from the carrying sibling");
        let album = post.album.as_ref().expect("album info present");
        assert_eq!(album.media_count, 8);
        assert_eq!(album.message_ids.len(), 8);
        assert_eq!(album.media_types.len(), 8);
        assert!(collapsed[1].album.is_none());
    }

    #[test]
    fn caption_on_a_later_sibling_still_wins() {
        let messages = vec![album_member(11, 5, ""), album_member(12, 5, "late caption")];
        let collapsed = collapse_albums(messages);
        assert_eq!(collapsed[0].id.get(), 11);
        assert_eq!(collapsed[0].text, "late caption");
    }

    #[test]
    fn lone_album_member_stays_plain() {
        let collapsed = collapse_albums(vec![album_member(5, 99, "caption")]);
        assert_eq!(collapsed.len(), 1);
        assert!(collapsed[0].album.is_none(), "an album of one is noise");
    }

    #[test]
    fn post_counter_admits_siblings_beyond_limit() {
        let mut c = PostCounter::default();
        assert!(c.admit(Some((100, 7)), 1), "first sibling starts post 1");
        assert!(
            c.admit(Some((100, 7)), 1),
            "sibling of an admitted album is free"
        );
        assert!(!c.admit(None, 1), "a single would start post 2 — stop");
        assert!(
            !c.admit(Some((100, 8)), 1),
            "a new album would start post 2 — stop"
        );
    }

    #[test]
    fn post_counter_counts_singles() {
        let mut c = PostCounter::default();
        assert!(c.admit(None, 2));
        assert!(c.admit(None, 2));
        assert!(!c.admit(None, 2));
    }

    #[test]
    fn post_counter_reports_overflow_only_after_refusal() {
        let mut counter = PostCounter::default();
        assert!(counter.admit(None, 2));
        assert!(counter.admit(None, 2));
        assert!(!counter.overflowed(), "no refusal yet");
        assert!(!counter.admit(None, 2), "third post must be refused");
        assert!(counter.overflowed());
    }

    #[test]
    fn same_grouped_id_in_different_channels_stays_separate_posts() {
        let mut a = create_test_message(10, "from channel A", 100);
        a.grouped_id = Some(777);
        let mut b = create_test_message(11, "from channel B", 200);
        b.grouped_id = Some(777);

        let collapsed = collapse_albums(vec![a, b]);

        assert_eq!(
            collapsed.len(),
            2,
            "same gid across channels must not merge"
        );
        assert!(
            collapsed.iter().all(|m| m.album.is_none()),
            "each is a lone member, stays plain"
        );
    }

    #[test]
    fn accumulator_admits_album_siblings_beyond_limit_and_latches_has_more() {
        let mut page = PageAccumulator::new(true, 1);
        assert!(page.push(album_member(1, 7, "caption")));
        assert!(
            page.push(album_member(2, 7, "")),
            "sibling passes beyond limit"
        );
        assert!(!page.push(create_test_message(3, "next post", 100)));
        assert!(page.has_more());
        assert_eq!(page.into_messages().len(), 1, "album collapsed to one post");
    }

    #[test]
    fn accumulator_without_collapse_refuses_at_limit() {
        let mut page = PageAccumulator::new(false, 2);
        assert!(page.push(create_test_message(1, "a", 100)));
        assert!(page.push(create_test_message(2, "b", 100)));
        assert!(!page.push(create_test_message(3, "c", 100)));
        assert!(page.has_more());
        assert_eq!(page.into_messages().len(), 2);
    }

    #[test]
    fn accumulator_reports_no_has_more_when_nothing_refused() {
        let mut page = PageAccumulator::new(true, 5);
        assert!(page.push(create_test_message(1, "a", 100)));
        assert!(!page.has_more());
    }
}
