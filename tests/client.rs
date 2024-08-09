#![allow(unused)]
use bdk_kyoto::logger::PrintLogger;
use bdk_wallet::chain::spk_client::FullScanResult;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;
use tokio::task;
use tokio::time;

use bdk_kyoto::builder::LightClientBuilder;
use bdk_kyoto::TrustedPeer;
use bdk_testenv::bitcoincore_rpc::RpcApi;
use bdk_testenv::bitcoind;
use bdk_testenv::bitcoind::BitcoinD;
use bdk_testenv::TestEnv;
use bdk_wallet::bitcoin::{Amount, Network};
use bdk_wallet::CreateParams;
use bdk_wallet::{KeychainKind, Wallet};

const EXTERNAL_DESCRIPTOR: &str = "tr([7d94197e/86'/1'/0']tpubDCyQVJj8KzjiQsFjmb3KwECVXPvMwvAxxZGCP9XmWSopmjW3bCV3wD7TgxrUhiGSueDS1MU5X1Vb1YjYcp8jitXc5fXfdC1z68hDDEyKRNr/0/*)";
const INTERNAL_DESCRIPTOR: &str = "tr([7d94197e/86'/1'/0']tpubDCyQVJj8KzjiQsFjmb3KwECVXPvMwvAxxZGCP9XmWSopmjW3bCV3wD7TgxrUhiGSueDS1MU5X1Vb1YjYcp8jitXc5fXfdC1z68hDDEyKRNr/1/*)";

fn wait_for_height(env: &TestEnv, height: u32) -> anyhow::Result<()> {
    while env.rpc_client().get_block_count()? < height as u64 {
        let _ = time::sleep(Duration::from_millis(256));
    }
    Ok(())
}

#[tokio::test]
async fn it_works() -> anyhow::Result<()> {
    let mut env = TestEnv::new()?;

    // workaround to enable compact filters
    let _ = env.rpc_client().stop()?;
    let mut conf = bitcoind::Conf::default();
    conf.args.push("-blockfilterindex=1");
    conf.args.push("-peerblockfilters=1");
    let bitcoind = BitcoinD::with_conf(bitcoind::downloaded_exe_path()?, &conf)?;
    env.bitcoind = bitcoind;

    let peer = env.bitcoind.params.rpc_socket;

    let miner = env
        .rpc_client()
        .get_new_address(None, None)?
        .assume_checked();

    let mut wallet = CreateParams::new(EXTERNAL_DESCRIPTOR, INTERNAL_DESCRIPTOR)
        .network(Network::Regtest)
        .create_wallet_no_persist()?;

    let addr = wallet.next_unused_address(KeychainKind::External).address;

    // build client
    let peer = TrustedPeer {
        ip: peer.ip().clone().into(),
        port: Some(peer.port()),
    };
    let (mut node, mut client) = LightClientBuilder::new(&wallet)
        .peers(vec![peer])
        .logger(Arc::new(PrintLogger::new()))
        .connections(1)
        .build();
        
    // run node
    if !node.is_running() {
        task::spawn(async move { node.run().await });
    }

    // mine blocks
    let _hashes = env.mine_blocks(101, Some(miner.clone()))?;
    wait_for_height(&env, 101)?;
    println!("Height: {}", env.rpc_client().get_block_count()?);
    
    // send tx
    let txid = env.send(&addr, Amount::from_btc(0.21)?)?;
    println!("Txid: {txid}");
    let _ = env.mine_blocks(1, Some(miner))?;
    wait_for_height(&env, 102)?;

    let _ = time::sleep(Duration::from_secs(10));

    // get update
    if let Some(update) = client.update().await {
        let FullScanResult {
            graph_update,
            chain_update,
            last_active_indices,
        } = update;
        dbg!(&graph_update);
        dbg!(chain_update.height());
        dbg!(&last_active_indices);
        // let _ = wallet.apply_update(update)?;
    }
    
    client.shutdown().await?;

    Ok(())
}
