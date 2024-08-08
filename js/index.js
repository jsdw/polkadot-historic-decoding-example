#!/usr/bin/env node

import { ApiPromise, WsProvider } from "@polkadot/api"
import { exitWithError } from "./utils.js"
import args from "args"

args
    .option('url', 'URL of the node to connect to')
    .option('block', 'Block number to obtain', undefined, b => parseInt(b, 10))

const flags = args.parse(process.argv)
const url = flags.url ?? "wss://polkadot-public-rpc.blockops.network/ws"
const blockNumber = flags.block ?? exitWithError("--block number needs to be provided")

console.log(`connecting to url: ${url}`)
console.log(`fetching block: ${blockNumber}`)

const wsProvider = new WsProvider(url);
const api = await ApiPromise.create({ provider: wsProvider })
const blockHash = await api.rpc.chain.getBlockHash(blockNumber)
const block = await api.rpc.chain.getBlock(blockHash)

for (const ex of block.block.extrinsics) {
  console.log(`${ex.method.section}.${ex.method.method}`)
  console.log(JSON.stringify(ex.method, undefined, "\t"))
}

// The quickest way to end the process; otherwise PJS keeps it open.
process.exit(0)

// TODO:
// - Tweak scale-value so we can print values with type info etc eg when failed to decode enough bytes.
//   - Maybe this also lets us hide large inputs eg 0x1234...4324
// - Figure out why 4 bytes leftover on block 29231.
// - Parallelise rust block decoder better
// - Make more resiliant to network failures ie try again if any network error obtaining blocks