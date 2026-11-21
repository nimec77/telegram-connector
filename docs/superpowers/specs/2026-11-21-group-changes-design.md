# Replayable group changes

TaxMax needs to observe a reply and its successive edits in either Telegram group namespace. It can then resume from its saved cursor after the HTTP client or connector restarts.

Use the existing account update reader and its ordered Telegram batches. The capture adapter filters by configured group and current membership, writes the batch to a private SQLite journal, then lets the reader persist its Telegram position. A restart or detected discontinuity records a replayable gap. Telegram can omit intermediate edits while the upstream connection is down.

An HTTP page belongs to a linked generation and typed Telegram group. Require an API bearer with the read scope and an explicit group grant. Validate current membership before serving and recheck authorization after a wait. Retention advances the starting boundary and an expired cursor returns HTTP 410 with that boundary. Group capture starts when the service enables a joined group and uses the existing account budget, independent of HTTP readers.

The shared reader preserves a single live Telegram path and ordering. A dedicated Telegram session would compete with the existing account lifecycle and use a second connection. History polling cannot reconstruct intermediate edits or attribute deletions reliably.
