// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.0;

/**
 * @title TitanStaking
 * @dev Pacyte Nexus Titan Node Staking Kontratı
 * Minimum Stake: 1,000,000 PNX
 */
contract TitanStaking {
    IERC20 public immutable pnxToken;
    uint256 public constant MIN_STAKE = 1_000_000 * 10**18;
    
    // Slashing yetkilisi (Sovereign Vault veya Konsensüs kontratı)
    address public slashingAuthority;
    
    struct Validator {
        address owner;
        uint256 stake;
        uint256 joinedAt;
        bool isActive;
        bytes publicKey;
    }
    
    mapping(address => Validator) public validators;
    address[] public validatorList;
    
    event ValidatorRegistered(address indexed owner, uint256 stake, bytes publicKey);
    event ValidatorUnregistered(address indexed owner);
    event StakeIncreased(address indexed owner, uint256 amount);
    event StakeDecreased(address indexed owner, uint256 amount);
    event Slashed(address indexed owner, uint256 amount, string reason);
    event SlashingAuthoritySet(address indexed authority);
    
    constructor(address _pnxToken, address _slashingAuthority) {
        require(_pnxToken != address(0), "Invalid token");
        require(_slashingAuthority != address(0), "Invalid authority");
        pnxToken = IERC20(_pnxToken);
        slashingAuthority = _slashingAuthority;
        emit SlashingAuthoritySet(_slashingAuthority);
    }
    
    function registerValidator(bytes calldata publicKey) external {
        require(validators[msg.sender].owner == address(0), "Already registered");
        require(pnxToken.transferFrom(msg.sender, address(this), MIN_STAKE), "Transfer failed");
        
        validators[msg.sender] = Validator({
            owner: msg.sender,
            stake: MIN_STAKE,
            joinedAt: block.timestamp,
            isActive: true,
            publicKey: publicKey
        });
        
        validatorList.push(msg.sender);
        emit ValidatorRegistered(msg.sender, MIN_STAKE, publicKey);
    }
    
    function increaseStake(uint256 amount) external {
        Validator storage v = validators[msg.sender];
        require(v.isActive, "Not an active validator");
        require(pnxToken.transferFrom(msg.sender, address(this), amount), "Transfer failed");
        v.stake += amount;
        emit StakeIncreased(msg.sender, amount);
    }
    
    function unregister() external {
        Validator storage v = validators[msg.sender];
        require(v.isActive, "Not an active validator");
        v.isActive = false;
        require(pnxToken.transfer(msg.sender, v.stake), "Transfer failed");
        emit ValidatorUnregistered(msg.sender);
    }
    
    /**
     * @dev Sadece slashingAuthority tarafından çağrılabilir
     */
    function slash(address validator, uint256 amount, string calldata reason) external {
        require(msg.sender == slashingAuthority, "Only slashing authority");
        Validator storage v = validators[validator];
        require(v.isActive, "Validator not active");
        require(amount <= v.stake, "Amount exceeds stake");
        
        v.stake -= amount;
        
        if (v.stake < MIN_STAKE) {
            v.isActive = false;
        }
        
        emit Slashed(validator, amount, reason);
    }
    
    /**
     * @dev Slashing yetkilisini değiştir (sadece mevcut yetkili)
     */
    function setSlashingAuthority(address newAuthority) external {
        require(msg.sender == slashingAuthority, "Only slashing authority");
        require(newAuthority != address(0), "Invalid address");
        slashingAuthority = newAuthority;
        emit SlashingAuthoritySet(newAuthority);
    }
    
    function getValidatorCount() external view returns (uint256) {
        return validatorList.length;
    }
    
    function getActiveValidators() external view returns (address[] memory) {
        uint256 count = 0;
        for (uint256 i = 0; i < validatorList.length; i++) {
            if (validators[validatorList[i]].isActive) {
                count++;
            }
        }
        
        address[] memory active = new address[](count);
        uint256 index = 0;
        for (uint256 i = 0; i < validatorList.length; i++) {
            if (validators[validatorList[i]].isActive) {
                active[index] = validatorList[i];
                index++;
            }
        }
        return active;
    }
}