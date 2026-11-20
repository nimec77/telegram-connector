# Stdio Workspace Member Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax.

**Goal** — Put the stdio connector beside the other crates for the 0.27.0 release.

**Architecture** — The top-level Cargo manifest is a virtual workspace. The stdio package moves to `crates/telegram-connector` and remains the default member.

**Tech stack** — Rust 2024, Cargo resolver 3, pinned nightly and one workspace lockfile.

**Spec** — [Workspace specification](../../workspace-specification.md)

## Global constraints

- Preserve the `telegram-connector` package, `telegram_connector` imports, `telegram-mcp` binary, configuration, setup and both stdio protocols.
- Align first-party versions at 0.27.0. Preserve the external graph and Git pins.
- Validate on macOS using disposable offline fixtures.

## Task 1. Move the package and its tooling

- [x] Pin the relocated manifest identity in the version and dependency policy fixtures.
- [x] Move the manifest, stdio sources, tests and examples into `crates/telegram-connector`.
- [x] Verify a virtual workspace with the stdio default member, lockfile consistency and the unchanged external graph.

## Task 2. Update the guides and verify the member

- [x] Align installation, development and source links with the new paths.
- [x] Run the full workspace gate, isolated builds, member-path installation and four stdio probes.

## Task 3. Prepare the release

- [x] Verify workspace versions, changelog and README for the local release commit.


## Verification

The four local packages resolve at 0.27.0 with 435 external lock entries
unchanged. The virtual workspace defaults to the stdio member. The macOS gate
passed 1,294 Rust tests, three doctests, three browser tests, strict Clippy and
warning-free rustdoc. Four isolated release builds, a disposable member-path
installation, the four stdio probes, 31 policy fixtures and 20 version
fixtures passed.

Publish only the approved annotated `v0.27.0` tag after committing the release.
