# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/hectorvent/paloma/releases/tag/v0.1.0) - 2026-11-10

### Added

- *(graph)* default to Graph API v27.0
- *(send)* allow static WebP images in image messages
- *(cli)* add -f to every --file and document every argument
- *(cli)* share paging flags and print row ids for every list under -q
- *(calling)* add voicemail, icons by country and SRTP
- *(calling)* add call hours, permission and analytics
- estimate marketing reach and add Marketing Messages policy
- *(send)* send text, media and templates to a group
- *(group)* manage groups, invites, members, requests and pins
- *(webhook)* show and change the WhatsApp webhook version
- *(manifest)* add PhoneSettings and extend the Number kind
- *(flow)* add metrics, migration and endpoint key commands
- *(automation)* read a bot by its ID
- *(account)* add assigned users and welcome sequences
- *(commerce)* add cart and catalog commands
- *(number)* add onboarding, identity and settings commands
- *(cli)* finish the account and QR configuration workflows
- *(webhook)* show the active per-number callback override
- *(webhook)* add local receive tools and signed callback probes
- *(template)* add the advanced template lifecycle
- *(account)* add messaging and pricing analytics
- *(send)* route templates by their current category
- *(webhook)* support default and per-number callbacks
- *(template)* implement namespace reads
- *(automation)* add conversational automation commands
- *(qr)* add QR management and safe image downloads
- *(account)* add account control plane and billing migration
- *(send)* add flows, requests, address, carousel and products
- *(message)* add message history
- *(send)* add typed flags to send contact
- *(send)* accept BSUIDs as recipients and in the block list
- *(cli)* check media and profile input before sending
- *(send)* add interactive media headers and row descriptions
- *(template)* filter the list, delete in bulk, type create
- *(number)* read limits, page lists and pick a data region
- *(auth)* show token scopes and drop the refresh stub
- *(profile)* add profile photo
- *(media)* add resumable upload handles to media upload
- *(send)* add typed template flags, --reply-to and --voice
- *(cli)* read the PIN and verify token from env or stdin
- *(graph)* retry safe requests and exit 4 on throttling
- *(send)* add --messaging-account-id for multi-account tokens
- *(send)* add template components, named vars and header text
- *(cli)* add the command tree, output printer and app wiring

### Fixed

- *(number)* send NON_VISIBLE for a hidden number
- *(account)* read system users from the owning business
- accept parent BSUIDs in block and BSUIDs in group removal
- *(manifest)* check a Template shorthand only when it is submitted
- *(manifest)* guard and confirm apply like the matching commands
- *(graph)* resolve the app id in one place, exit 5 if missing
- tighten config, template and network error handling
- *(cli)* report what bulk writes did and close confirmation gaps
- *(manifest)* send only what changed on Template edits
- *(group)* show an empty group list as none
- match three response shapes seen on the live API
- *(graph)* classify 400 errors and keep Meta's explanation
- *(flow)* remove metrics, which Meta no longer serves
- tidy QR and welcome sequence output
- *(manifest)* make apply previews and writes match the commands
- *(flow)* warn that the metrics API is being deprecated
- *(number)* harden onboarding and identity commands
- *(template)* validate upsert languages before any request
- *(graph)* make dry-run URL placeholders explicit
- *(graph)* honor timeouts throughout media downloads
- *(send)* require a name part with send contact --name
- *(send)* stop claiming BSUID recipients are accepted
- *(manifest)* compare every declared template component
- *(graph)* redact secrets from previews and diagnostics

### Other

- *(cli)* keep SYSTEMROOT in the cleared test environment
- *(cli)* apply the grammar's last rules
- align contributor docs with App and add argument help
- tidy idioms and keep the cause of failures
- unit-test library modules and tie mocks to the API version
- *(cli)* resolve ids and adjust responses through App
- fix drift the docs audit found after the structure pass
- *(graph)* read responses and token details through api
- *(cli)* give each verb its own run
- move webhook probes and carousel checks, import explicitly
- split builders and validation by domain
- parse kinds once and keep crate internals private
- type template categories and calling settings
- *(cli)* carry values together instead of expecting them later
- *(cli)* move range checks to validate and share the JSON readers
- explain the PIN deprecation and which signatures count
- *(release)* ship the size-optimized dist build
- *(cli)* apply the grammar to auth, config and export
- *(cli)* apply the grammar to calling, group and qr
- *(cli)* apply the grammar to template, number and webhook
- remove the Java-build migration and secret value flags
- drop unused dependencies and dead code
- open every module with the constraint it exists for
- drop workspace-wide length and bool lint allows
- *(cli)* name repeated limits and split modules by family
- give bodies, lookups, error codes and profiles a home
- *(cli)* split contract tests into one module per group
- pass every cargo-deny check and license under MIT
- *(cli)* fix drift between help and the reference pages
- correct stale and missing command references
- add the README, architecture notes and the docs index
- *(cli)* add contract tests against a mocked Graph API
