use bdk_wallet::chain::spk_client::FullScanResult;
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
use bdk_wallet::KeychainKind;

const EXTERNAL_DESCRIPTOR: &str = "tr([7d94197e/86'/1'/0']tpubDCyQVJj8KzjiQsFjmb3KwECVXPvMwvAxxZGCP9XmWSopmjW3bCV3wD7TgxrUhiGSueDS1MU5X1Vb1YjYcp8jitXc5fXfdC1z68hDDEyKRNr/0/*)";
const INTERNAL_DESCRIPTOR: &str = "tr([7d94197e/86'/1'/0']tpubDCyQVJj8KzjiQsFjmb3KwECVXPvMwvAxxZGCP9XmWSopmjW3bCV3wD7TgxrUhiGSueDS1MU5X1Vb1YjYcp8jitXc5fXfdC1z68hDDEyKRNr/1/*)";

fn wait_for_height(env: &TestEnv, height: u32) -> anyhow::Result<()> {
    while env.rpc_client().get_block_count()? < height as u64 {
        let _ = time::sleep(Duration::from_millis(256));
    }
    Ok(())
}

#[tokio::test]
async fn update_returns_blockchain_data() -> anyhow::Result<()> {
    let mut env = TestEnv::new()?;

    // workaround to enable compact filters
    let _ = env.rpc_client().stop()?;
    let mut conf = bitcoind::Conf::default();
    conf.p2p = bitcoind::P2P::Yes;
    conf.args.push("-blockfilterindex=1");
    conf.args.push("-peerblockfilters=1");
    let bitcoind = BitcoinD::with_conf(bitcoind::downloaded_exe_path()?, &conf)?;
    env.bitcoind = bitcoind;

    let peer = env.bitcoind.params.p2p_socket.unwrap();

    let miner = env
        .rpc_client()
        .get_new_address(None, None)?
        .assume_checked();

    let wallet = CreateParams::new(EXTERNAL_DESCRIPTOR, INTERNAL_DESCRIPTOR)
        .network(Network::Regtest)
        .create_wallet_no_persist()?;

    let index = 2;
    let addr = wallet.peek_address(KeychainKind::External, index).address;

    // build client
    let peer = TrustedPeer {
        ip: peer.ip().clone().into(),
        port: Some(peer.port()),
    };
    let (mut node, mut client) = LightClientBuilder::new(&wallet)
        .peers(vec![peer])
        // .logger(Arc::new(PrintLogger::new()))
        .connections(1)
        .build();

    // mine blocks
    let _hashes = env.mine_blocks(101, Some(miner.clone()))?;
    wait_for_height(&env, 101)?;

    // send tx
    let amt = Amount::from_btc(0.21)?;
    let txid = env.send(&addr, amt)?;
    let hashes = env.mine_blocks(1, Some(miner))?;
    wait_for_height(&env, 102)?;

    // run node
    if !node.is_running() {
        task::spawn(async move { node.run().await });
    }

    // get update
    if let Some(update) = client.update().await {
        let FullScanResult {
            graph_update,
            chain_update,
            last_active_indices,
        } = update;
        // graph tx and anchor
        let tx_node = graph_update.full_txs().next().unwrap();
        assert_eq!(tx_node.txid, txid);
        let anchor = tx_node.anchors.first().unwrap();
        assert_eq!(anchor.block_id.height, 102);
        assert_eq!(anchor.block_id.hash, hashes[0]);
        let tx = tx_node.tx;
        let txout = tx.output.iter().find(|txout| txout.value == amt).unwrap();
        assert_eq!(txout.script_pubkey, addr.script_pubkey());
        // chain
        assert_eq!(chain_update.height(), 102);
        // keychain
        assert_eq!(
            last_active_indices,
            [(KeychainKind::External, index)].into()
        );
    }

    client.shutdown().await?;

    Ok(())
}
