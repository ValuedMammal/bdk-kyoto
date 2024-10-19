use bdk_kyoto::builder::LightClientBuilder;
use bdk_kyoto::{Event, LogLevel};
use bdk_wallet::bitcoin::Network;
use bdk_wallet::chain::BlockId;
use bdk_wallet::rusqlite;
use bdk_wallet::Wallet;
use kyoto::{ServiceFlags, TrustedPeer};
use std::net::{IpAddr, Ipv4Addr};

/* Sync a bdk wallet using events */

const DB_PATH: &str = "bdk-wallet.db";
const START_HEIGHT: u32 = 170_000;
const NETWORK: Network = Network::Signet;

const PEERS: &[IpAddr] = &[
    IpAddr::V4(Ipv4Addr::new(23, 137, 57, 100)),
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut db = rusqlite::Connection::open(DB_PATH)?;

    let desc = "tr([7d94197e/86'/1'/0']tpubDCyQVJj8KzjiQsFjmb3KwECVXPvMwvAxxZGCP9XmWSopmjW3bCV3wD7TgxrUhiGSueDS1MU5X1Vb1YjYcp8jitXc5fXfdC1z68hDDEyKRNr/0/*)";
    let change_desc = "tr([7d94197e/86'/1'/0']tpubDCyQVJj8KzjiQsFjmb3KwECVXPvMwvAxxZGCP9XmWSopmjW3bCV3wD7TgxrUhiGSueDS1MU5X1Vb1YjYcp8jitXc5fXfdC1z68hDDEyKRNr/1/*)";

    let mut wallet = match Wallet::load().load_wallet(&mut db)? {
        Some(wallet) => wallet,
        None => Wallet::create(desc, change_desc)
            .network(NETWORK)
            .create_wallet(&mut db)?,
    };

    let local_height = wallet.latest_checkpoint().height();

    let peers = PEERS
        .iter()
        .map(|ip| {
            let mut peer = TrustedPeer::from_ip(*ip);
            peer.set_services(ServiceFlags::P2P_V2);
            peer
        })
        .collect();

    // The light client builder handles the logic of inserting the SPKs
    let (node, mut client) = LightClientBuilder::new(&wallet)
        .peers(peers)
        .scan_after(local_height.max(START_HEIGHT))
        .build()
        .unwrap();

    tokio::task::spawn(async move { node.run().await });

    loop {
        if let Some(event) = client.next_event(LogLevel::Info).await {
            match event {
                Event::Log(log) => println!("INFO: {log}"),
                Event::Warning(warning) => println!("WARNING: {warning}"),
                Event::ScanResponse(full_scan_result) => {
                    wallet.apply_update(full_scan_result).unwrap();
                    println!("INFO: Balance: {}", wallet.balance().total(),);
                    let _ = wallet.persist(&mut db)?;
                }
                Event::PeersFound => println!("INFO: Connected to all necessary peers."),
                Event::TxSent(txid) => println!("INFO: Broadcast transaction: {txid}"),
                Event::TxFailed(failure_payload) => {
                    println!("WARNING: Transaction failed to broadcast: {failure_payload:?}")
                }
                Event::StateChange(node_state) => println!("NEW TASK: {node_state}"),
                Event::BlocksDisconnected(headers) => {
                    let first = headers
                        .iter()
                        .map(|h| BlockId {
                            height: h.height,
                            hash: h.header.block_hash(),
                        })
                        .min()
                        .unwrap();
                    let _ = wallet.disconnect_checkpoint(first);
                }
            }
        }
    }
}
