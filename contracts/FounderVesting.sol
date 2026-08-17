// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.0;

/**
 * @title FounderVesting
 * @dev Pacyte Nexus Kurucu Token Kilidi
 * 1 yıl cliff + 3 yıl linear vesting = Toplam 4 yıl
 */
contract FounderVesting {
    address public beneficiary;
    address public immutable token;
    uint256 public immutable startTime;
    
    uint256 public constant CLIFF = 365 days;
    uint256 public constant DURATION = 4 * 365 days;
    uint256 public constant TOTAL_ALLOCATION = 55_000_000 * 10**18;
    
    uint256 public released;
    
    event TokensReleased(uint256 amount);
    event BeneficiaryChanged(address indexed newBeneficiary);
    
    constructor(address _token, address _beneficiary) {
        require(_token != address(0), "Token address cannot be zero");
        require(_beneficiary != address(0), "Beneficiary cannot be zero");
        
        token = _token;
        beneficiary = _beneficiary;
        startTime = block.timestamp;
    }
    
    function releasableAmount() public view returns (uint256) {
        return vestedAmount() - released;
    }
    
    function vestedAmount() public view returns (uint256) {
        uint256 currentTime = block.timestamp;
        
        if (currentTime < startTime + CLIFF) {
            return 0;
        }
        
        if (currentTime >= startTime + DURATION) {
            return TOTAL_ALLOCATION;
        }
        
        return (TOTAL_ALLOCATION * (currentTime - startTime)) / DURATION;
    }
    
    function release() public {
        uint256 amount = releasableAmount();
        require(amount > 0, "No tokens to release");
        
        released += amount;
        require(IERC20(token).transfer(beneficiary, amount), "Transfer failed");
        
        emit TokensReleased(amount);
    }
    
    function changeBeneficiary(address newBeneficiary) public {
        require(msg.sender == beneficiary, "Only beneficiary");
        require(newBeneficiary != address(0), "Invalid address");
        beneficiary = newBeneficiary;
        emit BeneficiaryChanged(newBeneficiary);
    }
}

interface IERC20 {
    function transfer(address to, uint256 value) external returns (bool);
    function approve(address spender, uint256 value) external returns (bool);
    function transferFrom(address from, address to, uint256 value) external returns (bool);
    function balanceOf(address owner) external view returns (uint256);
    function allowance(address owner, address spender) external view returns (uint256);
}