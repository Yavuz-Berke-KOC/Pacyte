Pacyte Nexus (PNX)

A Titan-Grade, Hybrid Post-Quantum Layer 1 Blockchain

https://img.shields.io/badge/license-Apache%202.0-blue.svg
https://img.shields.io/badge/rust-2021%20edition-orange.svg
https://img.shields.io/badge/version-v0.25.2--alpha-brightgreen.svg
https://img.shields.io/badge/code-48%2C000%2B%20lines-blueviolet.svg

---

🚀 What is Pacyte Nexus?

Pacyte Nexus is a next-generation Layer-1 blockchain written entirely from scratch in Rust. It introduces Hardware Meritocracy (PoHM) — a consensus model where validators (Titans) are selected by verifiable hardware capability, not just economic stake. The protocol is secured by hybrid post-quantum cryptography (Ed25519 + NIST-standard Dilithium5), supports both EVM and WASM smart contracts, and features a built-in Sentinel Watcher Layer for decentralized community auditing.

No forks. No shortcuts. 48,000+ lines of original Rust code.

---

✨ Key Features

Feature Description
⚡ Hardware Meritocracy Real CPUID verification; mandatory AVX-512 for Titans
🔐 Hybrid Post-Quantum Ed25519 + Dilithium5 dual-signature on every transaction
🛡️ Sentinel Watcher Consumer-hardware community auditing with automated slashing
⚙️ Dual VM EVM (Solidity) + WASM (Rust, C, AssemblyScript)
🔥 Tri-Phase Deflation 550M → 250M fixed supply via Great Burn mechanism
⏱️ 1s Block Time HotStuff BFT consensus with 2-second finality
💾 RocksDB + WAL Zero data loss on power failure
🌐 REST + JSON-RPC + WS Full Ethereum RPC compatibility (MetaMask, Hardhat)

---

📦 Prerequisites

Tool Version Download
Rust 1.70+ rustup.rs
Git Any git-scm.com
LLVM 18.1.8 (Windows) LLVM Releases

LLVM Installation (Windows): Download LLVM-18.1.8-win64.exe, run the installer, and check "Add LLVM to the system PATH" during setup.

---

📦 Build & Run

```bash
# Clone the repository
git clone https://github.com/Yavuz-Berke-KOC/pacyte.git
cd pacyte

# Build in release mode
cargo build --release

# Run as Sentinel (Watcher) node
./target/release/pacyte-node --node-id 1 --port 9333

# Run as Titan (Validator) node
./target/release/pacyte-node --node-id 1 --port 9333 --validator
```

---

🧪 Testing the Node

Once the node is running, open a second terminal:

```bash
# Get current block number
curl -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
  http://localhost:9332

# Get Genesis Vault balance (122.5M PAC)
curl -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0x0000000000000000000000000000000000000000000000000000000000000000","latest"],"id":1}' \
  http://localhost:9332

# Get network information
curl -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"pacyte_getNetworkInfo","params":[],"id":1}' \
  http://localhost:9332
```

---

🌐 API Endpoints

Protocol Default Port Purpose
REST API 8080 Block queries, transaction submission, account lookups
JSON-RPC 9332 Ethereum-compatible (MetaMask, Hardhat, Web3.js)
WebSocket 9334 Real-time event subscriptions
P2P 9333 Peer-to-peer network communication

---

🏗️ Project Architecture

```
pacyte/
├── Cargo.toml
├── README.md
├── LICENSE
├── src/
│   ├── main.rs                   # Entry point
│   ├── lib.rs                    # Library root
│   ├── types/                    # Block, Transaction, Account, Error, Config
│   ├── crypto/                   # Ed25519, Dilithium5, Hybrid Signer, Hash, Merkle
│   ├── storage/                  # RocksDB, WAL, State Manager, Cache, Snapshot, Migration
│   ├── network/                  # P2P, Peer Manager, Message, Gossip, Handshake
│   ├── mempool/                  # Transaction Pool, Validator, Fee, Ordering, Pruning
│   ├── consensus/                # HotStuff Engine, Validator, Vote, Round, Safety, Pacemaker, Sentinel
│   ├── vault/                    # Vault Manager, Burn, Dormancy, Bridge, Fee, Sovereign
│   ├── execution/                # VM, Gas Calculator, Executor, Contract, WASM Runtime, Precompiles
│   ├── api/                      # REST Server, JSON-RPC, WebSocket, Middleware
│   └── utils/                    # Metrics (Prometheus), Logger, Config, Time
└── tests/                        # Integration, Network, Consensus, Benchmarks, Fuzz
```

---

📊 Project Status

Module Status
Types ✅ 0 errors
Crypto ✅ 0 errors
Storage ✅ 0 errors
Network ✅ 0 errors
Mempool ✅ 0 errors
Consensus ✅ 0 errors
Vault ✅ 0 errors
Execution ✅ 0 errors
API ✅ 0 errors
Utils ✅ 0 errors
Tests ✅ 0 errors

All 11 modules compile with zero errors. Node runs successfully.

---

🔧 Technical Specifications

Parameter Value
Language Rust (Edition 2021, zero forks)
Codebase 48,000+ lines
Consensus HotStuff BFT + PoHM
Block Time 1 second
Finality 2 seconds
Validators 21 Titans (scalable)
Quorum 2/3 + 1
Signature Scheme Ed25519 + Dilithium5 (NIST FIPS 204)
Hash Functions SHA-256, Keccak-256, BLAKE3
Smart Contracts EVM (140+ opcodes) + WASM
Storage Engine RocksDB with custom WAL
Initial Supply 550,000,000 PNX
Final Supply Cap 250,000,000 PNX
Minimum Stake 1,000,000 PNX

---

📚 Documentation

· Full Whitepaper (Google Drive)
https://drive.google.com/file/d/1FnCbahQBCtP6KJ90_l-yhkmHLsHVqajT/view?usp=drivesdk

---

🤝 Contributing

Pacyte Nexus is in active alpha development. Contributions are welcome!

1. Fork the repository
2. Create your feature branch (git checkout -b feature/amazing-feature)
3. Commit your changes (git commit -m 'Add amazing feature')
4. Push to the branch (git push origin feature/amazing-feature)
5. Open a Pull Request

---

📄 License

This project is licensed under the Apache 2.0 License. See the LICENSE file for details.

---

🌟 Support the Project

· ⭐ Star this repository
· 🐦 Follow @PacyteNexus on X (Twitter)
· 💬 Join our Discord (server coming soon)

---

Built with ❤️ by Yavuz Berke KOÇ

"Reward the Titan. Empower the Sentinel. Secure the Future."
