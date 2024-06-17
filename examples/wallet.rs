#![allow(unused)]

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

use bdk_chain::bitcoin::{BlockHash, Network, ScriptBuf};
use bdk_file_store::Store;
use bdk_wallet::bitcoin;
use bdk_wallet::wallet;
use bdk_wallet::KeychainKind;
use bdk_wallet::Wallet;

use kyoto::chain::checkpoints::HeaderCheckpoint;
use kyoto::node::builder::NodeBuilder;

// Sync a bdk wallet

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path = ".bdk_kyoto_wallet_example";
    let mut db = Store::<wallet::ChangeSet>::open_or_create_new(b"db_magic", db_path)?;
    let loaded = db.aggregate_changesets()?;

    let desc = "tr([7d94197e/86'/1'/0']tpubDCyQVJj8KzjiQsFjmb3KwECVXPvMwvAxxZGCP9XmWSopmjW3bCV3wD7TgxrUhiGSueDS1MU5X1Vb1YjYcp8jitXc5fXfdC1z68hDDEyKRNr/0/*)";
    let change_desc = "tr([7d94197e/86'/1'/0']tpubDCyQVJj8KzjiQsFjmb3KwECVXPvMwvAxxZGCP9XmWSopmjW3bCV3wD7TgxrUhiGSueDS1MU5X1Vb1YjYcp8jitXc5fXfdC1z68hDDEyKRNr/1/*)";

    let mut wallet = Wallet::new_or_load(desc, change_desc, loaded, bitcoin::Network::Signet)?;
    println!(
        "{}",
        wallet.next_unused_address(KeychainKind::External).address
    );
    println!(
        "Balance before sync: {} sats",
        wallet.balance().total().to_sat()
    );

    // Get revealed script pubkeys for each keychain, defaulting to 20 if none are revealed
    let mut spks: HashSet<ScriptBuf> = HashSet::new();
    for keychain in [KeychainKind::External, KeychainKind::Internal] {
        let last_reveal = match wallet.derivation_index(keychain) {
            Some(index) => index,
            None => {
                let target_index = 19;
                let _ = wallet.reveal_addresses_to(keychain, target_index);
                target_index
            }
        };
        for idx in 0..=last_reveal {
            let spk = wallet.spk_index().spk_at_index(keychain, idx).unwrap();
            spks.insert(spk.to_owned());
        }
    }

    // Get local chain tip
    let tip = wallet.latest_checkpoint().block_id();
    println!("Last local tip: {} {}", tip.height, tip.hash);

    //let header_cp = HeaderCheckpoint::new(tip.height, convert_hash(&tip.hash));
    let header_cp = HeaderCheckpoint::new(
        169_000,
        BlockHash::from_str("000000ed6fe89c46140f55ff511c558bcbdb1239ba95474f38f619b3bb657d4a")?,
    );

    // Configure kyoto node and console logger
    let subscriber = tracing_subscriber::FmtSubscriber::new();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let port = 38333;
    let peers = vec![
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        IpAddr::V4(Ipv4Addr::new(170, 75, 163, 219)),
        IpAddr::V4(Ipv4Addr::new(23, 137, 57, 100)),
    ];
    let builder = NodeBuilder::new(Network::Signet);
    let (mut node, client) = builder
        .add_peers(peers.into_iter().map(|ip| (ip, port)).collect())
        .add_scripts(spks)
        .anchor_checkpoint(header_cp)
        .num_required_peers(1)
        .build_node()
        .await;

    // Start a sync request
    let req = bdk_kyoto::Request::new(wallet.local_chain().tip(), wallet.spk_index());
    let mut client = req.into_client(client);

    // Run the node
    if !node.is_running() {
        tokio::task::spawn(async move { node.run().await });
    }

    println!("Syncing!");

    // Apply updates
    if let Some(update) = client.sync().await {
        let bdk_kyoto::Update {
            cp,
            indexed_tx_graph,
        } = update;

        wallet.apply_update(wallet::Update {
            chain: Some(cp),
            graph: indexed_tx_graph.graph().clone(),
            last_active_indices: indexed_tx_graph.index.last_used_indices(),
        })?;
    } else {
        println!("nothing to do");
    }

    // Persist changes
    if let Some(changeset) = wallet.take_staged() {
        db.append_changeset(&changeset)?;
    }

    let _ = client.shutdown().await?;

    let cp = wallet.latest_checkpoint();
    println!("Synced to tip: {} {}", cp.height(), cp.hash());
    println!(
        "Balance after sync: {} sats",
        wallet.balance().total().to_sat()
    );

    Ok(())
}
