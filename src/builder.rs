//! [`bdk_kyoto::Client`] builder

use core::fmt;
use std::{collections::HashSet, path::PathBuf, str::FromStr, sync::Arc};

use bdk_chain::{keychain::KeychainTxOutIndex, local_chain::CheckPoint, SpkIterator};
use kyoto::{
    chain::checkpoints::{
        HeaderCheckpoint, MAINNET_HEADER_CP, REGTEST_HEADER_CP, SIGNET_HEADER_CP,
    },
    node::{builder::NodeBuilder, node::Node},
    BlockHash, Network, ScriptBuf, TrustedPeer,
};

use crate::{logger::NodeMessageHandler, Client};

const TARGET_INDEX: u32 = 20;
const RECOMMENDED_PEERS: u8 = 2;

/// Construct a light client from higher level components.
#[derive(Debug)]
pub struct LightClientBuilder<'a, K> {
    cp: CheckPoint,
    index: &'a KeychainTxOutIndex<K>,
    peers: Option<Vec<TrustedPeer>>,
    connections: Option<u8>,
    birthday_height: Option<u32>,
    data_dir: Option<PathBuf>,
    message_handler: Option<Arc<dyn NodeMessageHandler>>,
}

impl<'a, K> LightClientBuilder<'a, K> {
    /// Construct a new node builder
    pub fn new(cp: CheckPoint, index: &'a KeychainTxOutIndex<K>) -> Self {
        Self {
            cp,
            index,
            peers: None,
            connections: None,
            birthday_height: None,
            data_dir: None,
            message_handler: None,
        }
    }
    /// Add peers to connect to over the P2P network.
    pub fn peers(mut self, peers: Vec<TrustedPeer>) -> Self {
        self.peers = Some(peers);
        self
    }

    /// Add the number of connections for the node to maintain.
    pub fn connections(mut self, num_connections: u8) -> Self {
        self.connections = Some(num_connections);
        self
    }

    /// Handle messages from the node
    pub fn logger(mut self, message_handler: Arc<dyn NodeMessageHandler>) -> Self {
        self.message_handler = Some(message_handler);
        self
    }

    /// Add a directory to store node data
    pub fn data_dir(mut self, dir: PathBuf) -> Self {
        self.data_dir = Some(dir);
        self
    }

    /// Add a wallet "birthday", or block to start searching for transactions _strictly after_.
    /// Only useful for recovering wallets. If the wallet has a tip that is already higher than the
    /// height provided, this height will be ignored.
    pub fn scan_after(mut self, height: u32) -> Self {
        self.birthday_height = Some(height);
        self
    }
}

impl<'a, K> LightClientBuilder<'a, K>
where
    K: fmt::Debug + Clone + Ord,
{
    /// Build light client with node configured for [`Network::Signet`].
    pub fn build_signet(self) -> (Node, Client<K>) {
        self._build(Network::Signet)
    }

    /// Build light client with node configured for [`Network::Bitcoin`].
    pub fn build(self) -> (Node, Client<K>) {
        self._build(Network::Bitcoin)
    }

    /// Build a light client node and a client to interact with the node
    fn _build(self, network: Network) -> (Node, Client<K>) {
        let mut node_builder = NodeBuilder::new(network);
        if let Some(whitelist) = self.peers {
            node_builder = node_builder.add_peers(whitelist);
        }
        let local_tip = self.cp.block_id();
        match self.birthday_height {
            Some(birthday) => {
                if birthday < local_tip.height {
                    let header_cp = HeaderCheckpoint::new(local_tip.height, local_tip.hash);
                    node_builder = node_builder.anchor_checkpoint(header_cp)
                } else {
                    let cp = get_checkpoint_for_height(birthday, &network);
                    node_builder = node_builder.anchor_checkpoint(cp)
                }
            }
            None => {
                if local_tip.height > 0 {
                    let header_cp = HeaderCheckpoint::new(local_tip.height, local_tip.hash);
                    node_builder = node_builder.anchor_checkpoint(header_cp)
                }
            }
        }
        if let Some(dir) = self.data_dir {
            node_builder = node_builder.add_data_dir(dir);
        }
        node_builder =
            node_builder.num_required_peers(self.connections.unwrap_or(RECOMMENDED_PEERS));
        let spks: HashSet<ScriptBuf> = self
            .index
            .keychains()
            .flat_map(|(keychain, desc)| {
                let target_idx = self
                    .index
                    .last_revealed_index(keychain)
                    .unwrap_or(TARGET_INDEX);
                SpkIterator::new_with_range(desc, 0..=target_idx).map(|(_i, spk)| spk)
            })
            .collect();
        let (node, kyoto_client) = node_builder.add_scripts(spks).build_node();
        let mut client = Client::from_index(self.cp, self.index, kyoto_client);
        if let Some(logger) = self.message_handler {
            client.set_logger(logger)
        }
        (node, client)
    }
}

fn get_checkpoint_for_height(height: u32, network: &Network) -> HeaderCheckpoint {
    let checkpoints: Vec<HeaderCheckpoint> = match network {
        Network::Bitcoin => MAINNET_HEADER_CP
            .iter()
            .copied()
            .map(|(height, hash)| HeaderCheckpoint::new(height, BlockHash::from_str(hash).unwrap()))
            .collect(),
        Network::Testnet => panic!(),
        Network::Signet => SIGNET_HEADER_CP
            .iter()
            .copied()
            .map(|(height, hash)| HeaderCheckpoint::new(height, BlockHash::from_str(hash).unwrap()))
            .collect(),
        Network::Regtest => REGTEST_HEADER_CP
            .iter()
            .copied()
            .map(|(height, hash)| HeaderCheckpoint::new(height, BlockHash::from_str(hash).unwrap()))
            .collect(),
        _ => unreachable!(),
    };
    let mut cp = *checkpoints.first().unwrap();
    for checkpoint in checkpoints {
        if height.ge(&checkpoint.height) {
            cp = checkpoint;
        } else {
            break;
        }
    }
    cp
}
