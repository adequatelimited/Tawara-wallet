// Copyright (c) 2026 Adequate Systems HK Limited
// Licensed under the Mochimo Cryptocurrency Engine License Agreement 1.0; see LICENSE.md.

//! The node the wallet queries.
//!
//! - **The user's node choice is kept.** Without a choice the wallet uses
//!   [`crate::preferences::DEFAULT_NODE`], and a node that was chosen, or
//!   forgotten, is never replaced by it. While no node is set (after Forget,
//!   or when a remembered one cannot be used), the worker opens a store
//!   without reconciling it and says why. No node is queried at start-up.
//! - **TLS is required.** `https://` is accepted, and `http://` only to the
//!   loopback interface (`127.0.0.0/8`, `::1`, `localhost`): the node's
//!   answers drive reconciliation and the amounts a spend is built from, and
//!   on a plaintext link anyone on the path can rewrite them. Unlike the
//!   command line, this wallet has no `--allow-plaintext-node` override.
//! - **Trust roots.** The library's transport is `ureq` over `rustls` with
//!   the bundled `webpki-roots`, as in the command-line wallet. Using the
//!   platform's roots instead would be a library change.
//!
//! The worker never builds a transport itself: it asks a [`Connect`], so the
//! tests can hand it a scripted chain and the application the real one,
//! [`HttpsNode`].

use core::fmt;

use mochimo_crypto::cli::args::plaintext_off_loopback;
use mochimo_crypto::mesh::Transport;
use mochimo_crypto::mesh::http::UreqTransport;

/// What builds a transport for a node URL. Building one opens no socket; a
/// URL it cannot use is refused here, before any password is asked for.
pub trait Connect: Send + 'static {
    /// The transport it builds.
    type Transport: Transport + Send + 'static;

    /// A transport for `url`. The worker calls it only for a URL that has
    /// passed its own rule (https, or http to the loopback interface).
    fn connect(&self, url: &str) -> Result<Self::Transport, mochimo_crypto::Error>;
}

/// The application's node: the library's `ureq` transport over TLS.
#[derive(Clone, Copy, Debug, Default)]
pub struct HttpsNode;

impl Connect for HttpsNode {
    type Transport = UreqTransport;

    fn connect(&self, url: &str) -> Result<UreqTransport, mochimo_crypto::Error> {
        UreqTransport::new(url)
    }
}

/// A node URL this wallet will not use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeRefused {
    /// Plaintext `http://` to a host that is not loopback.
    Plaintext { url: String },
    /// The library's transport refused the URL (scheme, authority).
    Malformed { url: String, why: String },
}

impl fmt::Display for NodeRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The command line's refusal (`cli::args::plaintext_node_refusal`,
            // private there), verbatim except where it names its own flags:
            // `--node {url}` is `{url}`, "`reconcile --advance-to`" is "an
            // acknowledged advance", and the ACTION line drops
            // `--allow-plaintext-node`, which this wallet does not have.
            NodeRefused::Plaintext { url } => write!(
                f,
                "{url} is plaintext http to a host that is not loopback, and everything this \
                 wallet decides comes from that link: the balance a spend is laid out against, \
                 the ledger address reconciliation compares its own record to, the chain tip a \
                 block-to-live is judged against, and the key position that says whether a key \
                 has already signed. Anyone on the path can rewrite all of it, and can read every \
                 tag you ask about.\n  A rewritten balance does not move funds -- the network \
                 checks send + change + fee against the ledger and rejects a transaction built on \
                 a lie -- but a rewritten reconciliation report is what an acknowledged advance \
                 acts on.\n  ACTION: use an https API. http to 127.0.0.0/8, ::1 or localhost is \
                 accepted."
            ),
            NodeRefused::Malformed { url, why } => write!(
                f,
                "cannot use the API {url}: {why}. An API is https://host or https://host:port, \
                 with no path, query or fragment."
            ),
        }
    }
}

impl std::error::Error for NodeRefused {}

/// The Mesh middleware's own sync state (`/network/status`'s
/// `sync_status`): how its last refresh of its one node's tip went.
///
/// It says nothing about whether that node is current with the network: a
/// node that has fallen behind answers its old tip, and the middleware,
/// having taken it, says it is synchronized. How long ago the tip was
/// solved is what shows a tip that has stopped moving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncState {
    /// The middleware's wording, made safe to show: `synchronized` once a
    /// refresh has finished, `synchronizing` while one takes a new tip, or
    /// the step that failed.
    pub stage: String,
    /// Whether its last refresh finished.
    pub synced: bool,
}

/// A network the node serves (`/network/list`), by name, each made safe to
/// show: the chain (`mochimo`) and the network (`mainnet`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkName {
    pub blockchain: String,
    pub network: String,
}

/// Check a node URL against the TLS rule and the library's transport, and
/// build its transport.
///
/// Plaintext is judged by the command line's own test
/// (`cli::args::plaintext_off_loopback`): `127.0.0.0/8`, `::1` and the name
/// `localhost` are loopback, and any other name is not, whatever it resolves
/// to. A scheme that is neither is left to the transport, which refuses it.
pub(crate) fn check_node_url<C: Connect>(
    connect: &C,
    url: &str,
) -> Result<C::Transport, NodeRefused> {
    let url = url.trim();
    if plaintext_off_loopback(url) {
        return Err(NodeRefused::Plaintext {
            url: url.to_owned(),
        });
    }
    connect.connect(url).map_err(|e| NodeRefused::Malformed {
        url: url.to_owned(),
        why: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_is_the_only_plaintext_accepted() {
        for ok in [
            "https://node.example",
            "https://node.example:8443/",
            "http://127.0.0.1:8080",
            "http://127.9.9.9",
            "http://[::1]:8080",
            "http://LOCALHOST/",
        ] {
            assert!(!plaintext_off_loopback(ok), "{ok}");
        }
        for bad in [
            "http://node.example",
            "http://10.0.0.1",
            "http://127.0.0.1.example.com",
            "http://[2001:db8::1]:80",
            "http://[::1",
        ] {
            assert!(plaintext_off_loopback(bad), "{bad}");
        }
    }

    #[test]
    fn the_https_node_is_checked_by_the_library() {
        assert!(check_node_url(&HttpsNode, "https://node.example").is_ok());
        assert!(check_node_url(&HttpsNode, " http://127.0.0.1:2095 ").is_ok());
        assert!(matches!(
            check_node_url(&HttpsNode, "http://node.example"),
            Err(NodeRefused::Plaintext { .. })
        ));
        assert!(matches!(
            check_node_url(&HttpsNode, "https://node.example/path"),
            Err(NodeRefused::Malformed { .. })
        ));
        assert!(matches!(
            check_node_url(&HttpsNode, "ftp://x"),
            Err(NodeRefused::Malformed { .. })
        ));
    }

    #[test]
    fn the_plaintext_refusal_names_no_flag() {
        let text = NodeRefused::Plaintext {
            url: "http://x".into(),
        }
        .to_string();
        assert!(!text.contains("--allow-plaintext-node"), "{text}");
        assert!(text.contains("use an https API"), "{text}");
    }
}
