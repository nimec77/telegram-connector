# GitHub version tags

Read this after the four-file consistency check and Rust gate pass. New tags use
the repository convention: annotated `v<package-version>`, message `Release v<version>`.
Publishing a tag sends its reachable commits to GitHub; it does not update a branch
or create a GitHub Release. Existing version tags are immutable.

## Select the destination and release commit

1. Use the user-selected remote, otherwise `origin`. Inspect its effective push
   destinations with `git remote get-url --push --all <remote>` and verify the
   GitHub owner/repository with `gh repo view OWNER/REPO --json nameWithOwner,url`.
   Require one intended push destination; resolve multiple configured destinations
   before publication. Read remote tag state from that push repository, which may
   differ from the fetch repository. Do not change the user's remote configuration.
   Use the installed `gh` skill: GitHub reads use `gh`; Git commits/tags/pushes use `git`.
2. Inspect both local `refs/tags/v<target>` and the exact remote tag before creating
   anything. `gh api repos/OWNER/REPO/git/ref/tags/v<target>` returns the remote ref;
   an annotated tag's object type is `tag`, so follow its SHA with
   `gh api repos/OWNER/REPO/git/tags/TAG_OBJECT_SHA` until reaching a commit. A 404
   means absent only after confirming access to the repository; authentication or
   network failures leave publication pending.
3. For a new bump, commit only its intended changes after validation, using a
   conventional message such as `chore: bump version to <target>`. Preserve unrelated
   staged/unstaged edits, including edits in the same README or changelog file.
   Use a temporary index or hunk-scoped staging/commit when needed; the resulting
   commit includes only the intended bump, and original unrelated staging remains
   intact. Avoid a blanket `git add` or committing an unrelated existing index.
   Record the full commit SHA.
   When the session already requests a local merge, finish it before selecting the
   validated release commit. Otherwise the bump commit is the release commit;
   tag publication does not authorize an additional merge or branch push.
4. For an already-bumped version, inspect existing tags before choosing a commit.
   Reuse a valid remote tag's commit, or a valid unpushed local tag's commit. Later
   unrelated commits on HEAD do not require moving an existing tag. If no tag exists,
   use the validated current commit; first commit any required documentation repair.
5. Verify the selected commit contains the target `[package].version`, matching
   root lock version, dated changelog section, and README version/link. For an
   existing tag, read its four committed files with `git show` (fetch the exact tag
   if needed), copy only those files to a temporary directory, and run the bundled
   `scripts/check_version_docs.py` against that directory. Use `--allow-older-release`
   only for an explicitly requested historical downgrade. Never tag uncommitted
   metadata or assume that an annotated tag object SHA is the commit SHA.
   A missing tag's publication requires the Rust gate on that exact source revision;
   if reusing an older unpushed local tag, validate its checkout in a temporary
   worktree instead of relying on the current HEAD's gate result.

## Publish exactly one immutable tag

| Observed state | Action |
| --- | --- |
| Neither local nor remote tag exists | Create an annotated tag at the validated release commit, then push it. |
| Only the local tag exists | Verify its commit and push that existing tag. |
| Remote tag exists at the verified release commit | Report it as already published; no push is needed. |
| Existing tag has the wrong version/files, local and remote tags peel to different commits, or a new bump's tag points elsewhere | Stop tag publication and report the conflicting SHAs. Preserve both refs. |

For new tags, set `version_tag`, `release_commit`, and `tag_remote` from the verified
values above, then run:

```bash
git tag -a "$version_tag" "$release_commit" -m "Release $version_tag"
git push --no-follow-tags "$tag_remote" "refs/tags/$version_tag:refs/tags/$version_tag"
```

For an existing local tag, skip creation and push the same explicit refspec with
`--no-follow-tags`. This overrides `push.followTags=true`, which otherwise also
publishes unrelated annotated tags even with an explicit refspec. Use neither
`--tags` nor `--follow-tags`; never force-push, delete, or move an existing tag.
If push fails or times out, inspect the remote ref and peeled commit again before
retrying. If it already matches, publication succeeded; if it conflicts, stop. A
still-absent tag can be retried after correcting the reported transport failure.

Verify the final tag through `gh api` and peel annotated objects to the validated
commit. Report the tag URL `https://github.com/OWNER/REPO/tree/v<target>`, commit SHA,
and created/already-present/pending status. Do not claim success from a local tag
or push exit status alone.
