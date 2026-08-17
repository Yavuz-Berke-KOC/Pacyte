// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.0;

/**
 * @title PNXToken
 * @dev Pacyte Nexus Yerel Tokenı (ERC-20)
 * Toplam Arz: 550,000,000 PNX
 */
interface IERC20 {
    function totalSupply() external view returns (uint256);
    function balanceOf(address account) external view returns (uint256);
    function transfer(address recipient, uint256 amount) external returns (bool);
    function allowance(address owner, address spender) external view returns (uint256);
    function approve(address spender, uint256 amount) external returns (bool);
    function transferFrom(address sender, address recipient, uint256 amount) external returns (bool);
    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
}

contract PNXToken is IERC20 {
    string public name = "Pacyte Nexus";
    string public symbol = "PNX";
    uint8 public decimals = 18;
    uint256 public override totalSupply = 550_000_000 * 10**18;
    
    mapping(address => uint256) public override balanceOf;
    mapping(address => mapping(address => uint256)) public override allowance;
    
    // Kontrat sahibi (deploy eden)
    address public immutable owner;
    
    // Dağıtım tamamlandı mı?
    bool public distributionComplete;
    
    // Titan ödülleri için dağıtılan toplam miktar
    uint256 public totalRewardsDistributed;
    
    event DistributionCompleted();
    event TitanRewardDistributed(address indexed titan, uint256 amount);
    
    constructor() {
        owner = msg.sender;
        balanceOf[address(this)] = totalSupply;
        emit Transfer(address(0), address(this), totalSupply);
    }
    
    /**
     * @dev Genesis dağıtımını yap (sadece bir kere, sadece owner)
     * @param _founderVesting FounderVesting kontrat adresi
     * @param _sovereignVault Sovereign Vault adresi
     * @param _ecosystemVault Ecosystem & Liquidity adresi
     */
    function executeGenesisDistribution(
        address _founderVesting,
        address _sovereignVault,
        address _ecosystemVault
    ) external {
        require(!distributionComplete, "Distribution already completed");
        require(msg.sender == owner, "Only owner");
        require(_founderVesting != address(0), "Invalid founder vesting");
        require(_sovereignVault != address(0), "Invalid sovereign vault");
        require(_ecosystemVault != address(0), "Invalid ecosystem vault");
        
        uint256 founderAmount = 55_000_000 * 10**18;
        uint256 sovereignAmount = 122_500_000 * 10**18;
        uint256 ecosystemAmount = 122_500_000 * 10**18;
        uint256 totalToDistribute = founderAmount + sovereignAmount + ecosystemAmount;
        
        require(balanceOf[address(this)] >= totalToDistribute, "Insufficient reserve");
        
        distributionComplete = true;
        
        // 1. Founder & Core Team: 55M
        balanceOf[address(this)] -= founderAmount;
        balanceOf[_founderVesting] += founderAmount;
        emit Transfer(address(this), _founderVesting, founderAmount);
        
        // 2. Sovereign Vault (Treasury): 122.5M
        balanceOf[address(this)] -= sovereignAmount;
        balanceOf[_sovereignVault] += sovereignAmount;
        emit Transfer(address(this), _sovereignVault, sovereignAmount);
        
        // 3. Ecosystem & Liquidity: 122.5M
        balanceOf[address(this)] -= ecosystemAmount;
        balanceOf[_ecosystemVault] += ecosystemAmount;
        emit Transfer(address(this), _ecosystemVault, ecosystemAmount);
        
        // Kontratta 250M kalır (Titan ödülleri için rezerv)
        
        emit DistributionCompleted();
    }
    
    /**
     * @dev Titan ödülü dağıt (sadece owner, her blokta çağrılabilir)
     * @param _titan Titan adresi
     * @param _amount Ödül miktarı
     */
    function distributeTitanReward(address _titan, uint256 _amount) external {
        require(msg.sender == owner, "Only owner");
        require(_titan != address(0), "Invalid titan address");
        require(_amount > 0, "Amount must be > 0");
        require(balanceOf[address(this)] >= _amount, "Insufficient reserve");
        
        balanceOf[address(this)] -= _amount;
        balanceOf[_titan] += _amount;
        totalRewardsDistributed += _amount;
        
        emit Transfer(address(this), _titan, _amount);
        emit TitanRewardDistributed(_titan, _amount);
    }
    
    /**
     * @dev Kontrattaki rezerv miktarını göster
     */
    function reserveBalance() public view returns (uint256) {
        return balanceOf[address(this)];
    }
    
    function transfer(address to, uint256 value) external override returns (bool) {
        require(to != address(0), "Invalid recipient");
        require(balanceOf[msg.sender] >= value, "Insufficient balance");
        balanceOf[msg.sender] -= value;
        balanceOf[to] += value;
        emit Transfer(msg.sender, to, value);
        return true;
    }
    
    function approve(address spender, uint256 value) external override returns (bool) {
        allowance[msg.sender][spender] = value;
        emit Approval(msg.sender, spender, value);
        return true;
    }
    
    function transferFrom(address from, address to, uint256 value) external override returns (bool) {
        require(balanceOf[from] >= value, "Insufficient balance");
        require(allowance[from][msg.sender] >= value, "Insufficient allowance");
        balanceOf[from] -= value;
        balanceOf[to] += value;
        allowance[from][msg.sender] -= value;
        emit Transfer(from, to, value);
        return true;
    }
    
    /**
     * @dev Allowance'ı güvenli şekilde artır (front-running koruması)
     */
    function increaseAllowance(address spender, uint256 addedValue) external returns (bool) {
        allowance[msg.sender][spender] += addedValue;
        emit Approval(msg.sender, spender, allowance[msg.sender][spender]);
        return true;
    }
    
    /**
     * @dev Allowance'ı güvenli şekilde azalt (front-running koruması)
     */
    function decreaseAllowance(address spender, uint256 subtractedValue) external returns (bool) {
        uint256 currentAllowance = allowance[msg.sender][spender];
        require(currentAllowance >= subtractedValue, "Decreased allowance below zero");
        allowance[msg.sender][spender] = currentAllowance - subtractedValue;
        emit Approval(msg.sender, spender, allowance[msg.sender][spender]);
        return true;
    }
}