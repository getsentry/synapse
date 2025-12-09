//! Relay Project Configuration Handler
//!
//! This module implements the split-merge strategy for handling Sentry's
//! `/api/0/relays/projectconfigs/` endpoint across multiple upstream Sentry instances
//! in a multi-cell architecture.
//!
//! # Protocol Overview
//!
//! The Relay Project Configs endpoint (version 3) is used by Relay instances to fetch
//! project configurations needed to process events. This implementation acts as an
//! aggregation layer that:
//! 1. Splits requests across multiple upstream Sentry cells based on project ownership
//! 2. Fans out requests in parallel to multiple upstreams
//! 3. Merges responses back into a single v3-compliant response
//! 4. Passes through all config data unchanged
//!
//! ## Endpoint Details
//! The endpoint implementation is at <https://github.com/getsentry/sentry/blob/master/src/sentry/api/endpoints/relay/project_configs.py>
//!
//! - **Path**: `/api/0/relays/projectconfigs/`
//! - **Method**: `POST`
//! - **Protocol Version**: 3
//! - **Authentication**: RelayAuthentication (X-Sentry-Relay-Id, X-Sentry-Relay-Signature)
//!
//! # Request Format (Version 3)
//!
//! ```json
//! {
//!   "publicKeys": ["key1", "key2", "key3"],
//!   "noCache": false,
//!   "global": true
//! }
//! ```
//!
//! ## Request Fields
//!
//! - **`publicKeys`** (required): Array of project public keys (DSN keys) to fetch configs for
//! - **`noCache`** (optional): If `true`, bypass caching and compute fresh configs (downgrades to v2 behavior)
//! - **`global`** (optional): If `true`, include global relay configuration in response
//!
//! # Response Format (Version 3)
//!
//! ```json
//! {
//!   "configs": {
//!     "key1": {
//!       "disabled": false,
//!       "slug": "project-slug",
//!       "publicKeys": [...],
//!       "config": {...},
//!       "organizationId": 1,
//!       "projectId": 42
//!     }
//!   },
//!   "pending": ["key2", "key3"],
//!   "global": {
//!     "measurements": {...},
//!     "options": {...}
//!   },
//!   "global_status": "ready"
//! }
//! ```
//!
//! ## Response Fields
//!
//! - **`configs`**: Map of public keys to their project configurations
//!   - Configs are passed through unchanged from upstream Sentry instances
//!   - They will add the relevant processing relay URL in the response
//! - **`pending`**: Array of public keys for which configs are being computed asynchronously
//!   - Relay should retry the request later to fetch these configs
//!   - Also used when upstreams fail/timeout (graceful degradation)
//! - **`global`**: Global relay configuration (only if `global: true` in request)
//! - **`global_status`**: Status of global config (always "ready" when present)
//!
//! # Implementation Details
//!
//! ## Request Splitting Strategy
//!
//! When a request arrives with multiple public keys:
//!
//! 1. **Route each key to its owning cell**
//!    - Query locator service for each public key to get cell name
//!
//! 2. **Group keys by target upstream**
//!    - Keys routed to the same cell are batched into one request
//!
//! 3. **Handle global config flag**
//!    - All upstreams receive the same `global` flag value as the original request
//!    - Global config is selected from the highest priority cell that returns it
//!    - Priority is determined by cell order in configuration (first = highest priority)
//!    - Enables failover: if highest priority cell fails, next cell's global config is used
//!
//! ## Response Merging Strategy
//!
//! Responses from multiple upstreams are merged as follows:
//!
//! ### Configs (HashMap merge)
//! - Merge all `configs` HashMaps from all upstreams
//! - Configs are passed through unchanged (no modifications)
//! - relay_url is expected to be added in the upstream response
//!
//! ### Pending (Array concatenation)
//! - Concatenate all `pending` arrays from all upstream responses
//! - Include keys from failed/timed-out upstreams
//! - Relay will retry these keys in a subsequent request
//!
//! ### Extra fields (Priority-based selection)
//! - Select `extra` fields from highest priority cell that responds successfully
//! - Priority determined by cell order in configuration (first = highest)
//! - Forward compatibility: new fields are automatically preserved
//!
//! ## Error Handling
//!
//! ### Partial Failures (Graceful Degradation)
//! - If some upstreams succeed: Return successful configs + pending for failed keys
//! - Failed keys are added to `pending` array (v3 protocol)
//! - Logged but does not block response
//!
//! ### Total Failure
//! - If all upstreams fail: Check if any keys were added to pending
//! - If pending is not empty: Return 200 OK with pending array (relay will retry)
//! - If pending is empty: Return 503 error (no recoverable state)
//!
//! ### Upstream Failure Scenarios
//! - **Timeout**: All keys from that upstream → pending
//! - **Connection error**: All keys from that upstream → pending
//! - **Parse error**: All keys from that upstream → pending
//! - **Task panic**: Logged error (extreme edge case, keys lost)
//!
//! ## Request Flow
//!
//! ### Success Scenario
//!
//! ```text
//! ┌─────────────┐
//! │   Relay     │
//! └──────┬──────┘
//!        │
//!        │ POST /api/0/relays/projectconfigs/
//!        │ {publicKeys: [A,B,C,D,E,F]}
//!        │
//!        ▼
//! ┌──────────────────────────────────────┐
//! │      Ingest Router (this handler)    │
//! │                                      │
//! │  1. Parse request                    │
//! │  2. Split keys by cell:              │
//! │     • US1 → [A,C,E]                  │
//! │     • US2 → [B,D,F]                  │
//! └───┬──────────────────────────┬───────┘
//!     │                          │
//!     │ {publicKeys: [A,C,E],    │ {publicKeys: [B,D,F],
//!     │  global: true}           │  global: true}
//!     │                          │
//!     ▼                          ▼
//! ┌──────────┐              ┌──────────┐
//! │Cell US1  │              │Cell US2  │
//! │(Sentry)  │              │(Sentry)  │
//! └────┬─────┘              └─────┬────┘
//!      │                          │
//!      │ {configs: {A,C,E}}       │ {configs: {B,D,F}}
//!      │                          │
//!      └──────────┬───────────────┘
//!                 ▼
//! ┌──────────────────────────────────────┐
//! │      Ingest Router (this handler)    │
//! │                                      │
//! │  3. Merge responses:                 │
//! │     • Combine all configs            │
//! │     • Merge pending arrays           │
//! │     • Select others from priority    │
//! └──────────────┬───────────────────────┘
//!                │
//!                │ {configs: {A,B,C,D,E,F},
//!                │  global: {...}}
//!                │
//!                ▼
//!         ┌─────────────┐
//!         │   Relay     │
//!         └─────────────┘
//! ```
//!
//! ### Failure Scenario (Graceful Degradation)
//!
//! When an upstream fails or times out, keys are added to the `pending` array:
//!
//! ```text
//! ┌─────────────┐
//! │   Relay     │
//! └──────┬──────┘
//!        │
//!        │ POST {publicKeys: [A,B,C,D,E,F]}
//!        │
//!        ▼
//! ┌──────────────────────────────────────┐
//! │      Ingest Router (this handler)    │
//! │  Split: US1→[A,C,E], US2→[B,D,F]     │
//! └───┬──────────────────────────┬───────┘
//!     │                          │
//!     ▼                          ▼
//! ┌──────────┐              ┌──────────┐
//! │Cell US1  │              │Cell US2  │
//! └────┬─────┘              └─────┬────┘
//!      │                          │
//!      │ ✓ Success                │ ✗ Timeout/Error
//!      │ {configs: {A,C,E}}       │
//!      │                          │
//!      └──────────┬───────────────┘
//!                 ▼
//! ┌──────────────────────────────────────┐
//! │      Ingest Router (this handler)    │
//! │                                      │
//! │  • Collect successful: {A,C,E}       │
//! │  • Add failed to pending: [B,D,F]    │
//! └──────────────┬───────────────────────┘
//!                │
//!                │ {configs: {A,C,E},
//!                │  pending: [B,D,F]}
//!                │
//!                ▼
//!         ┌─────────────┐
//!         │   Relay     │ (will retry pending)
//!         └─────────────┘
//! ```
//!
//! # Examples
//!
//! ## Example 1: All upstreams succeed
//!
//! **Request**:
//! ```json
//! {"publicKeys": ["key1", "key2"]}
//! ```
//!
//! **Response**:
//! ```json
//! {
//!   "configs": {
//!     "key1": {"disabled": false, "slug": "project-us1", ...},
//!     "key2": {"disabled": false, "slug": "project-us2", ...}
//!   }
//! }
//! ```
//!
//! ## Example 2: One upstream fails
//!
//! **Request**:
//! ```json
//! {"publicKeys": ["key1", "key2", "key3"]}
//! ```
//!
//! **Response** (if upstream with key2,key3 times out):
//! ```json
//! {
//!   "configs": {
//!     "key1": {"disabled": false, "slug": "project-us1", ...}
//!   },
//!   "pending": ["key2", "key3"]
//! }
//! ```
//!
//! ## Example 3: Request with global config
//!
//! **Request**:
//! ```json
//! {"publicKeys": ["key1", "key2"], "global": true}
//! ```
//!
//! **Splitting**:
//! - Request to US1: `{"publicKeys": ["key1"], "global": true}`
//! - Request to US2: `{"publicKeys": ["key2"], "global": true}`
//!
//! (US1 has higher priority, so its global config will be used in the final response)
//!
//! **Response**:
//! ```json
//! {
//!   "configs": {...},
//!   "global": {"measurements": {...}},
//!   "global_status": "ready"
//! }
//! ```

mod handler;
pub mod protocol;

pub use handler::ProjectConfigsHandler;
