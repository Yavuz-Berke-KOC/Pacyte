// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.0;

/**
 * @title BridgeVault
 * @dev Pacyte Nexus Cross-Shard Bridge Vault
 */
contract BridgeVault {
    IERC20 public immutable pnxToken;
    
    struct BridgeRequest {
        address sender;
        address recipient;
        uint256 amount;
        uint256 sourceShard;
        uint256 targetShard;
        uint256 timestamp;
        uint256 expiry;
        bool completed;
        bool reverted;
    }
    
    mapping(bytes32 => BridgeRequest) public requests;
    uint256 public constant TIMEOUT = 60; // 60 saniye
    
    event BridgeInitiated(bytes32 indexed requestId, address sender, address recipient, uint256 amount, uint256 sourceShard, uint256 targetShard);
    event BridgeCompleted(bytes32 indexed requestId);
    event BridgeReverted(bytes32 indexed requestId);
    
    constructor(address _pnxToken) {
        pnxToken = IERC20(_pnxToken);
    }
    
    function initiateBridge(address recipient, uint256 amount, uint256 targetShard) external returns (bytes32) {
        require(amount > 0, "Amount must be > 0");
        require(pnxToken.transferFrom(msg.sender, address(this), amount), "Transfer failed");
        
        bytes32 requestId = keccak256(abi.encodePacked(msg.sender, recipient, amount, block.timestamp));
        
        requests[requestId] = BridgeRequest({
            sender: msg.sender,
            recipient: recipient,
            amount: amount,
            sourceShard: block.chainid,
            targetShard: targetShard,
            timestamp: block.timestamp,
            expiry: block.timestamp + TIMEOUT,
            completed: false,
            reverted: false
        });
        
        emit BridgeInitiated(requestId, msg.sender, recipient, amount, block.chainid, targetShard);
        return requestId;
    }
    
    function completeBridge(bytes32 requestId) external {
        BridgeRequest storage req = requests[requestId];
        require(!req.completed, "Already completed");
        require(!req.reverted, "Already reverted");
        // Düzeltildi: req.expiry'den küçük veya eşitse hâlâ geçerli
        require(block.timestamp <= req.expiry, "Expired");
        
        req.completed = true;
        // Hedef shard'a token'ları gönder
        require(pnxToken.transfer(req.recipient, req.amount), "Transfer failed");
        emit BridgeCompleted(requestId);
    }
    
    function revertBridge(bytes32 requestId) external {
        BridgeRequest storage req = requests[requestId];
        require(!req.completed, "Already completed");
        require(!req.reverted, "Already reverted");
        // Düzeltildi: req.expiry'den büyükse timeout olmuş
        require(block.timestamp > req.expiry, "Not expired yet");
        
        req.reverted = true;
        require(pnxToken.transfer(req.sender, req.amount), "Refund failed");
        emit BridgeReverted(requestId);
    }
}