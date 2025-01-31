use std::net::{IpAddr, Ipv4Addr};

use bdk_kyoto::builder::{LightClientBuilder, ServiceFlags, TrustedPeer};
use bdk_kyoto::logger::TraceLogger;
use bdk_kyoto::{EventSenderExt, LightClient};
use bdk_wallet::bitcoin::Network;
use bdk_wallet::{KeychainKind, Wallet};

/// Peer address whitelist
const PEERS: &[IpAddr] = &[IpAddr::V4(Ipv4Addr::new(23, 137, 57, 100))];

/* Sync a bdk wallet */

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let desc = "tr([7d94197e/86'/1'/0']tpubDCyQVJj8KzjiQsFjmb3KwECVXPvMwvAxxZGCP9XmWSopmjW3bCV3wD7TgxrUhiGSueDS1MU5X1Vb1YjYcp8jitXc5fXfdC1z68hDDEyKRNr/0/*)";
    let change_desc = "tr([7d94197e/86'/1'/0']tpubDCyQVJj8KzjiQsFjmb3KwECVXPvMwvAxxZGCP9XmWSopmjW3bCV3wD7TgxrUhiGSueDS1MU5X1Vb1YjYcp8jitXc5fXfdC1z68hDDEyKRNr/1/*)";

    let peers = PEERS
        .iter()
        .map(|ip| {
            let mut peer = TrustedPeer::from_ip(*ip);
            peer.set_services(ServiceFlags::P2P_V2);
            peer
        })
        .collect();

    let mut wallet = Wallet::create(desc, change_desc)
        .network(Network::Signet)
        .lookahead(30)
        .create_wallet_no_persist()?;

    // The light client builder handles the logic of inserting the SPKs
    let LightClient {
        sender,
        mut receiver,
        node,
    } = LightClientBuilder::new()
        .scan_after(170_000)
        .peers(peers)
        .build(&wallet)
        .unwrap();

    tokio::task::spawn(async move { node.run().await });

    // Print logs to the console using the `tracing` crate
    let logger = TraceLogger::new()?;

    tracing::info!(
        "Balance before sync: {} sats",
        wallet.balance().total().to_sat()
    );

    // Sync and apply updates. We can do this a continual loop while the "application" is running.
    // Often this would occur on a separate thread than the underlying application user interface.
    loop {
        if let Some(update) = receiver.update(&logger).await {
            wallet.apply_update(update)?;
            tracing::info!("Tx count: {}", wallet.transactions().count());
            tracing::info!("Balance: {}", wallet.balance().total().to_sat());
            let last_revealed = wallet.derivation_index(KeychainKind::External);
            tracing::info!("Last revealed External: {:?}", last_revealed);
            tracing::info!(
                "Last revealed Internal: {:?}",
                wallet.derivation_index(KeychainKind::Internal)
            );
            tracing::info!("Local chain tip: {}", wallet.local_chain().tip().height());
            let next = wallet.reveal_next_address(KeychainKind::External).address;
            tracing::info!("Next receiving address: {next}");
            tracing::info!(
                "Broadcast minimum fee rate: {}",
                receiver.broadcast_minimum()
            );
            sender.add_revealed_scripts(&wallet).await?;
        }
    }
}
