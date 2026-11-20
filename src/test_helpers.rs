//! Shared offline fixtures for connector tests.

pub use telegram_core::test_helpers::{
    ManualClock, create_empty_search_result, create_test_channel, create_test_channel_detailed,
    create_test_channel_named, create_test_jpeg, create_test_message,
    create_test_message_with_enriched_forward, create_test_message_with_link_preview,
    create_test_search_result, permissive_limiter, raw_tl_channel, raw_tl_community,
    raw_tl_message, raw_tl_message_grouped, raw_tl_messages_slice, raw_tl_user,
};
