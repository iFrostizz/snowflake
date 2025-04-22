# Snowflake: Avalanche light node

<div class="imageandtext" style="display: flex; height: 280px">
    <img src="./media/snowflake.png" height="280px" alt="snowflake">
    <p class="imageflexcontent" style="margin-left: 20px; vertical-align: middle">
        Run an Avalanche node to interact with the Avalanche network with very small hardware and time requirements.
        <br>This node can be used to quickly spin up a client that will directly communicate with peers on the Avalanche network.
        <br>Since it's a particularly low consumption node, it can be used on low-end hardware such as phones, Raspberry Pi,
        or even browsers for instant access to the P2P network.
    </p>
</div>

## Ethereum JSON-RPC API (WIP)

| Method                                  | Supported |
|-----------------------------------------|-----------|
| web3_clientVersion                      | ❌         |
| web3_sha3                               | ❌         |
| net_version                             | ❌         |
| net_listening                           | ❌         |
| net_peerCount                           | ❌         |
| eth_protocolVersion                     | ❌         |
| eth_syncing                             | ❌         |
| eth_coinbase                            | ❌         |
| eth_chainId                             | ❌         |
| eth_mining                              | ❌         |
| eth_hashrate                            | ❌         |
| eth_gasPrice                            | ❌         |
| eth_accounts                            | ❌         |
| eth_blockNumber                         | ❌         |
| eth_getBalance                          | ❌         |
| eth_getStorageAt                        | ❌         |
| eth_getTransactionCount                 | ❌         |
| eth_getBlockTransactionCountByHash      | ❌         |
| eth_getBlockTransactionCountByNumber    | ❌         |
| eth_getUncleCountByBlockHash            | ❌         |
| eth_getUncleCountByBlockNumber          | ❌         |
| eth_getCode                             | ❌         |
| eth_sign                                | ❌         |
| eth_signTransaction                     | ❌         |
| eth_sendTransaction                     | ✅         |
| eth_sendRawTransaction                  | ❌         |
| eth_call                                | ❌         |
| eth_estimateGas                         | ❌         |
| eth_getBlockByHash                      | ❌         |
| eth_getBlockByNumber                    | ❌         |
| eth_getTransactionByHash                | ❌         |
| eth_getTransactionByBlockHashAndIndex   | ❌         |
| eth_getTransactionByBlockNumberAndIndex | ❌         |
| eth_getTransactionReceipt               | ❌         |
| eth_getUncleByBlockHashAndIndex         | ❌         |
| eth_getUncleByBlockNumberAndIndex       | ❌         |
| eth_newFilter                           | ❌         |
| eth_newBlockFilter                      | ❌         |
| eth_newPendingTransactionFilter         | ❌         |
| eth_uninstallFilter                     | ❌         |
| eth_getFilterChanges                    | ❌         |
| eth_getFilterLogs                       | ❌         |
| eth_getLogs                             | ❌         |

# Development
## Testing
### Docker
```sh
docker compose -f docker/docker-compose.yml build
docker compose -f docker/docker-compose.yml up
```