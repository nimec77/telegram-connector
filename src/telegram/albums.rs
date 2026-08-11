//! Album grouping: post-level limit counting and album collapsing (B5+A2).

use crate::telegram::types::{AlbumInfo, Message};
use std::collections::{HashMap, HashSet};

/// Counts posts (albums count once) while a fetch loop admits messages.
#[derive(Debug, Default)]
pub(crate) struct PostCounter {
    seen_groups: HashSet<i64>,
    posts: usize,
}

impl PostCounter {
    /// Admit a message into a window capped at `limit` posts. Returns `false`
    /// when the message would START a post beyond the limit — the caller
    /// stops fetching. Siblings of an already-admitted album are always
    /// admitted, so an album is never cut at the limit boundary (A2).
    pub(crate) fn admit(&mut self, grouped_id: Option<i64>, limit: usize) -> bool {
        if let Some(gid) = grouped_id
            && self.seen_groups.contains(&gid)
        {
            return true;
        }
        if self.posts >= limit {
            return false;
        }
        self.posts += 1;
        if let Some(gid) = grouped_id {
            self.seen_groups.insert(gid);
        }
        true
    }
}

/// Collapse album siblings (same `grouped_id`) into one post-level `Message`
/// (B5). Order-preserving on each group's first occurrence. The representative
/// is the lowest-id sibling (stable referencing); `text` comes from whichever
/// sibling carries it. A group with a single member in the window stays plain.
pub(crate) fn collapse_albums(messages: Vec<Message>) -> Vec<Message> {
    enum Slot {
        Single(Box<Message>),
        Group(i64),
    }

    let mut slots = Vec::new();
    let mut buckets: HashMap<i64, Vec<Message>> = HashMap::new();
    for msg in messages {
        match msg.grouped_id {
            Some(gid) => {
                let bucket = buckets.entry(gid).or_default();
                if bucket.is_empty() {
                    slots.push(Slot::Group(gid));
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
            Slot::Group(gid) => {
                let mut siblings = buckets.remove(&gid)?;
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
        assert!(c.admit(Some(7), 1), "first sibling starts post 1");
        assert!(c.admit(Some(7), 1), "sibling of an admitted album is free");
        assert!(!c.admit(None, 1), "a single would start post 2 — stop");
        assert!(
            !c.admit(Some(8), 1),
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
}
