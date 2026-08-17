// ===================================================================
// PACYTE NEXUS - SANAL MAKİNE (VM)
// ===================================================================

use std::collections::{HashMap, VecDeque};
use sha3::{Digest, Keccak256};

use crate::types::{Address, Hash, PacyteResult};
use super::{ExecutionError, ExecutionContext, GasMeter, GasSchedule, Log, StateChange};

// ===================================================================
// VM BELLEK
// ===================================================================

#[derive(Debug, Clone)]
pub struct Memory {
    data: Vec<u8>,
    last_gas_cost: u64,
}

impl Memory {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            last_gas_cost: 0,
        }
    }
    
    pub fn size(&self) -> usize {
        self.data.len()
    }
    
    pub fn resize(&mut self, new_size: usize) {
        if new_size > self.data.len() {
            self.data.resize(new_size, 0);
        }
    }
    
    pub fn read(&self, offset: usize, size: usize) -> Vec<u8> {
        if offset >= self.data.len() {
            return vec![0; size];
        }
        
        let end = (offset + size).min(self.data.len());
        let mut result = self.data[offset..end].to_vec();
        
        if result.len() < size {
            result.resize(size, 0);
        }
        
        result
    }
    
    pub fn read_word(&self, offset: usize) -> [u8; 32] {
        let bytes = self.read(offset, 32);
        let mut word = [0u8; 32];
        word.copy_from_slice(&bytes);
        word
    }
    
    pub fn write(&mut self, offset: usize, data: &[u8]) {
        let end = offset + data.len();
        if end > self.data.len() {
            self.resize(end);
        }
        self.data[offset..end].copy_from_slice(data);
    }
    
    pub fn write_word(&mut self, offset: usize, word: &[u8; 32]) {
        self.write(offset, word);
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

// ===================================================================
// VM STACK
// ===================================================================

const MAX_STACK_SIZE: usize = 1024;

#[derive(Debug, Clone)]
pub struct Stack {
    data: Vec<[u8; 32]>,
}

impl Stack {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }
    
    pub fn push(&mut self, value: [u8; 32]) -> Result<(), ExecutionError> {
        if self.data.len() >= MAX_STACK_SIZE {
            return Err(ExecutionError::StackOverflow);
        }
        self.data.push(value);
        Ok(())
    }
    
    pub fn pop(&mut self) -> Result<[u8; 32], ExecutionError> {
        self.data.pop().ok_or(ExecutionError::StackUnderflow)
    }
    
    pub fn peek(&self, depth: usize) -> Result<[u8; 32], ExecutionError> {
        if depth >= self.data.len() {
            return Err(ExecutionError::StackUnderflow);
        }
        Ok(self.data[self.data.len() - 1 - depth])
    }
    
    pub fn dup(&mut self, depth: usize) -> Result<(), ExecutionError> {
        let value = self.peek(depth)?;
        self.push(value)
    }
    
    pub fn swap(&mut self, depth: usize) -> Result<(), ExecutionError> {
        let len = self.data.len();
        if depth >= len {
            return Err(ExecutionError::StackUnderflow);
        }
        self.data.swap(len - 1, len - 1 - depth);
        Ok(())
    }
    
    pub fn len(&self) -> usize {
        self.data.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}

// ===================================================================
// OPCODES
// ===================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    // Stop
    Stop = 0x00,
    
    // Arithmetic
    Add = 0x01,
    Mul = 0x02,
    Sub = 0x03,
    Div = 0x04,
    SDiv = 0x05,
    Mod = 0x06,
    SMod = 0x07,
    AddMod = 0x08,
    MulMod = 0x09,
    Exp = 0x0a,
    SignExtend = 0x0b,
    
    // Comparison & Bitwise
    Lt = 0x10,
    Gt = 0x11,
    SLt = 0x12,
    SGt = 0x13,
    Eq = 0x14,
    IsZero = 0x15,
    And = 0x16,
    Or = 0x17,
    Xor = 0x18,
    Not = 0x19,
    Byte = 0x1a,
    Shl = 0x1b,
    Shr = 0x1c,
    Sar = 0x1d,
    
    // SHA3
    Sha3 = 0x20,
    
    // Environment
    Address = 0x30,
    Balance = 0x31,
    Origin = 0x32,
    Caller = 0x33,
    CallValue = 0x34,
    CallDataLoad = 0x35,
    CallDataSize = 0x36,
    CallDataCopy = 0x37,
    CodeSize = 0x38,
    CodeCopy = 0x39,
    GasPrice = 0x3a,
    ExtCodeSize = 0x3b,
    ExtCodeCopy = 0x3c,
    RetDataSize = 0x3d,
    RetDataCopy = 0x3e,
    ExtCodeHash = 0x3f,
    
    // Block
    BlockHash = 0x40,
    Coinbase = 0x41,
    Timestamp = 0x42,
    Number = 0x43,
    Difficulty = 0x44,
    GasLimit = 0x45,
    ChainId = 0x46,
    SelfBalance = 0x47,
    BaseFee = 0x48,
    
    // Memory/Storage/Flow
    Pop = 0x50,
    MLoad = 0x51,
    MStore = 0x52,
    MStore8 = 0x53,
    SLoad = 0x54,
    SStore = 0x55,
    Jump = 0x56,
    JumpI = 0x57,
    PC = 0x58,
    MSize = 0x59,
    Gas = 0x5a,
    JumpDest = 0x5b,
    
    // Push
    Push1 = 0x60,
    Push2 = 0x61,
    Push3 = 0x62,
    Push4 = 0x63,
    Push5 = 0x64,
    Push6 = 0x65,
    Push7 = 0x66,
    Push8 = 0x67,
    Push9 = 0x68,
    Push10 = 0x69,
    Push11 = 0x6a,
    Push12 = 0x6b,
    Push13 = 0x6c,
    Push14 = 0x6d,
    Push15 = 0x6e,
    Push16 = 0x6f,
    Push17 = 0x70,
    Push18 = 0x71,
    Push19 = 0x72,
    Push20 = 0x73,
    Push21 = 0x74,
    Push22 = 0x75,
    Push23 = 0x76,
    Push24 = 0x77,
    Push25 = 0x78,
    Push26 = 0x79,
    Push27 = 0x7a,
    Push28 = 0x7b,
    Push29 = 0x7c,
    Push30 = 0x7d,
    Push31 = 0x7e,
    Push32 = 0x7f,
    
    // Dup
    Dup1 = 0x80,
    Dup2 = 0x81,
    Dup3 = 0x82,
    Dup4 = 0x83,
    Dup5 = 0x84,
    Dup6 = 0x85,
    Dup7 = 0x86,
    Dup8 = 0x87,
    Dup9 = 0x88,
    Dup10 = 0x89,
    Dup11 = 0x8a,
    Dup12 = 0x8b,
    Dup13 = 0x8c,
    Dup14 = 0x8d,
    Dup15 = 0x8e,
    Dup16 = 0x8f,
    
    // Swap
    Swap1 = 0x90,
    Swap2 = 0x91,
    Swap3 = 0x92,
    Swap4 = 0x93,
    Swap5 = 0x94,
    Swap6 = 0x95,
    Swap7 = 0x96,
    Swap8 = 0x97,
    Swap9 = 0x98,
    Swap10 = 0x99,
    Swap11 = 0x9a,
    Swap12 = 0x9b,
    Swap13 = 0x9c,
    Swap14 = 0x9d,
    Swap15 = 0x9e,
    Swap16 = 0x9f,
    
    // Log
    Log0 = 0xa0,
    Log1 = 0xa1,
    Log2 = 0xa2,
    Log3 = 0xa3,
    Log4 = 0xa4,
    
    // System
    Create = 0xf0,
    Call = 0xf1,
    CallCode = 0xf2,
    Return = 0xf3,
    DelegateCall = 0xf4,
    Create2 = 0xf5,
    StaticCall = 0xfa,
    Revert = 0xfd,
    Invalid = 0xfe,
    SelfDestruct = 0xff,
}

impl Opcode {
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Opcode::Stop),
            0x01 => Some(Opcode::Add),
            0x02 => Some(Opcode::Mul),
            0x03 => Some(Opcode::Sub),
            0x04 => Some(Opcode::Div),
            0x05 => Some(Opcode::SDiv),
            0x06 => Some(Opcode::Mod),
            0x07 => Some(Opcode::SMod),
            0x08 => Some(Opcode::AddMod),
            0x09 => Some(Opcode::MulMod),
            0x0a => Some(Opcode::Exp),
            0x0b => Some(Opcode::SignExtend),
            
            0x10 => Some(Opcode::Lt),
            0x11 => Some(Opcode::Gt),
            0x12 => Some(Opcode::SLt),
            0x13 => Some(Opcode::SGt),
            0x14 => Some(Opcode::Eq),
            0x15 => Some(Opcode::IsZero),
            0x16 => Some(Opcode::And),
            0x17 => Some(Opcode::Or),
            0x18 => Some(Opcode::Xor),
            0x19 => Some(Opcode::Not),
            0x1a => Some(Opcode::Byte),
            0x1b => Some(Opcode::Shl),
            0x1c => Some(Opcode::Shr),
            0x1d => Some(Opcode::Sar),
            
            0x20 => Some(Opcode::Sha3),
            
            0x30 => Some(Opcode::Address),
            0x31 => Some(Opcode::Balance),
            0x32 => Some(Opcode::Origin),
            0x33 => Some(Opcode::Caller),
            0x34 => Some(Opcode::CallValue),
            0x35 => Some(Opcode::CallDataLoad),
            0x36 => Some(Opcode::CallDataSize),
            0x37 => Some(Opcode::CallDataCopy),
            0x38 => Some(Opcode::CodeSize),
            0x39 => Some(Opcode::CodeCopy),
            0x3a => Some(Opcode::GasPrice),
            0x3b => Some(Opcode::ExtCodeSize),
            0x3c => Some(Opcode::ExtCodeCopy),
            0x3d => Some(Opcode::RetDataSize),
            0x3e => Some(Opcode::RetDataCopy),
            0x3f => Some(Opcode::ExtCodeHash),
            
            0x40 => Some(Opcode::BlockHash),
            0x41 => Some(Opcode::Coinbase),
            0x42 => Some(Opcode::Timestamp),
            0x43 => Some(Opcode::Number),
            0x44 => Some(Opcode::Difficulty),
            0x45 => Some(Opcode::GasLimit),
            0x46 => Some(Opcode::ChainId),
            0x47 => Some(Opcode::SelfBalance),
            0x48 => Some(Opcode::BaseFee),
            
            0x50 => Some(Opcode::Pop),
            0x51 => Some(Opcode::MLoad),
            0x52 => Some(Opcode::MStore),
            0x53 => Some(Opcode::MStore8),
            0x54 => Some(Opcode::SLoad),
            0x55 => Some(Opcode::SStore),
            0x56 => Some(Opcode::Jump),
            0x57 => Some(Opcode::JumpI),
            0x58 => Some(Opcode::PC),
            0x59 => Some(Opcode::MSize),
            0x5a => Some(Opcode::Gas),
            0x5b => Some(Opcode::JumpDest),
            
            0x60..=0x7f => Some(Opcode::Push1),
	    0x80..=0x8f => Some(Opcode::Dup1),
	    0x90..=0x9f => Some(Opcode::Swap1),
	    0xa0..=0xa4 => Some(Opcode::Log0),
            
            0xf0 => Some(Opcode::Create),
            0xf1 => Some(Opcode::Call),
            0xf2 => Some(Opcode::CallCode),
            0xf3 => Some(Opcode::Return),
            0xf4 => Some(Opcode::DelegateCall),
            0xf5 => Some(Opcode::Create2),
            0xfa => Some(Opcode::StaticCall),
            0xfd => Some(Opcode::Revert),
            0xfe => Some(Opcode::Invalid),
            0xff => Some(Opcode::SelfDestruct),
            
            _ => None,
        }
    }
    
    pub fn gas_cost(&self, schedule: &GasSchedule) -> u64 {
        match self {
            Opcode::Stop => 0,
            Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div | Opcode::Mod |
            Opcode::Lt | Opcode::Gt | Opcode::Eq | Opcode::IsZero |
            Opcode::And | Opcode::Or | Opcode::Xor | Opcode::Not |
            Opcode::Pop | Opcode::PC | Opcode::MSize | Opcode::Gas |
            Opcode::Address | Opcode::Caller | Opcode::Origin |
            Opcode::CallDataSize | Opcode::CodeSize | Opcode::RetDataSize => schedule.base,
            
            Opcode::SDiv | Opcode::SMod | Opcode::SignExtend |
            Opcode::SLt | Opcode::SGt | Opcode::Byte |
            Opcode::Shl | Opcode::Shr | Opcode::Sar => schedule.low,
            
            Opcode::AddMod | Opcode::MulMod | Opcode::Jump | Opcode::JumpI => schedule.mid,
            
            Opcode::Exp => schedule.exp,
            Opcode::Sha3 => schedule.sha3,
            Opcode::SLoad => schedule.sload,
            Opcode::SStore => schedule.sstore_set,
            Opcode::Balance | Opcode::ExtCodeSize | Opcode::ExtCodeHash => schedule.balance,
            Opcode::Call | Opcode::CallCode | Opcode::DelegateCall | Opcode::StaticCall => schedule.call,
            Opcode::Create | Opcode::Create2 => schedule.create,
            Opcode::SelfDestruct => schedule.selfdestruct,            
            Opcode::MLoad | Opcode::MStore | Opcode::MStore8 => schedule.very_low,
            
                        
            Opcode::JumpDest => 1,
            Opcode::Invalid => 0,
            Opcode::Return | Opcode::Revert => 0,
            
            _ => schedule.mid,
        }
    }
}

// ===================================================================
// VM
// ===================================================================

pub struct VM {
    code: Vec<u8>,
    pc: usize,
    stack: Stack,
    memory: Memory,
    gas_meter: GasMeter,
    gas_schedule: GasSchedule,
    return_data: Vec<u8>,
    stopped: bool,
}

impl VM {
    pub fn new(code: Vec<u8>, gas_limit: u64) -> Self {
        Self {
            code,
            pc: 0,
            stack: Stack::new(),
            memory: Memory::new(),
            gas_meter: GasMeter::new(gas_limit),
            gas_schedule: GasSchedule::default(),
            return_data: Vec::new(),
            stopped: false,
        }
    }
    
    pub fn with_schedule(code: Vec<u8>, gas_limit: u64, schedule: GasSchedule) -> Self {
        Self {
            code,
            pc: 0,
            stack: Stack::new(),
            memory: Memory::new(),
            gas_meter: GasMeter::new(gas_limit),
            gas_schedule: schedule,
            return_data: Vec::new(),
            stopped: false,
        }
    }
    
    pub fn run(&mut self) -> Result<Vec<u8>, ExecutionError> {
        while !self.stopped && self.pc < self.code.len() {
            self.step()?;
        }
        
        Ok(self.return_data.clone())
    }
    
    fn step(&mut self) -> Result<(), ExecutionError> {
        let opcode_byte = self.code[self.pc];
        let opcode = Opcode::from_u8(opcode_byte)
            .ok_or(ExecutionError::InvalidOpcode(opcode_byte))?;
        
        // Gas kontrolü
        let gas_cost = opcode.gas_cost(&self.gas_schedule);
        if !self.gas_meter.use_gas(gas_cost) {
            return Err(ExecutionError::OutOfGas {
                used: self.gas_meter.used(),
                limit: self.gas_meter.gas_limit,
            });
        }
        
        self.pc += 1;
        
        match opcode {
            Opcode::Stop => {
                self.stopped = true;
            }
            
            Opcode::Add => {
                let a = self.stack.pop()?;
                let b = self.stack.pop()?;
                let result = add(&a, &b);
                self.stack.push(result)?;
            }
            
            Opcode::Mul => {
                let a = self.stack.pop()?;
                let b = self.stack.pop()?;
                let result = mul(&a, &b);
                self.stack.push(result)?;
            }
            
            Opcode::Sub => {
                let a = self.stack.pop()?;
                let b = self.stack.pop()?;
                let result = sub(&a, &b);
                self.stack.push(result)?;
            }
            
            Opcode::Div => {
                let a = self.stack.pop()?;
                let b = self.stack.pop()?;
                let result = div(&a, &b);
                self.stack.push(result)?;
            }
            
            Opcode::Lt => {
                let a = self.stack.pop()?;
                let b = self.stack.pop()?;
                let result = if lt(&a, &b) { word_one() } else { word_zero() };
                self.stack.push(result)?;
            }
            
            Opcode::Gt => {
                let a = self.stack.pop()?;
                let b = self.stack.pop()?;
                let result = if gt(&a, &b) { word_one() } else { word_zero() };
                self.stack.push(result)?;
            }
            
            Opcode::Eq => {
                let a = self.stack.pop()?;
                let b = self.stack.pop()?;
                let result = if a == b { word_one() } else { word_zero() };
                self.stack.push(result)?;
            }
            
            Opcode::IsZero => {
                let a = self.stack.pop()?;
                let result = if is_zero(&a) { word_one() } else { word_zero() };
                self.stack.push(result)?;
            }
            
            Opcode::And => {
                let a = self.stack.pop()?;
                let b = self.stack.pop()?;
                let mut result = [0u8; 32];
                for i in 0..32 {
                    result[i] = a[i] & b[i];
                }
                self.stack.push(result)?;
            }
            
            Opcode::Or => {
                let a = self.stack.pop()?;
                let b = self.stack.pop()?;
                let mut result = [0u8; 32];
                for i in 0..32 {
                    result[i] = a[i] | b[i];
                }
                self.stack.push(result)?;
            }
            
            Opcode::Xor => {
                let a = self.stack.pop()?;
                let b = self.stack.pop()?;
                let mut result = [0u8; 32];
                for i in 0..32 {
                    result[i] = a[i] ^ b[i];
                }
                self.stack.push(result)?;
            }
            
            Opcode::Not => {
                let a = self.stack.pop()?;
                let mut result = [0u8; 32];
                for i in 0..32 {
                    result[i] = !a[i];
                }
                self.stack.push(result)?;
            }
            
            Opcode::Pop => {
                self.stack.pop()?;
            }
            
            Opcode::MLoad => {
                let offset = word_to_usize(&self.stack.pop()?);
                let value = self.memory.read_word(offset);
                self.stack.push(value)?;
            }
            
            Opcode::MStore => {
                let offset = word_to_usize(&self.stack.pop()?);
                let value = self.stack.pop()?;
                self.memory.write_word(offset, &value);
            }
            
            Opcode::MStore8 => {
                let offset = word_to_usize(&self.stack.pop()?);
                let value = self.stack.pop()?;
                self.memory.write(offset, &[value[31]]);
            }
            
                        
                        
            Opcode::Jump => {
                let dest = word_to_usize(&self.stack.pop()?);
                if dest >= self.code.len() || self.code[dest] != 0x5b {
                    return Err(ExecutionError::InvalidJump);
                }
                self.pc = dest;
            }
            
            Opcode::JumpI => {
                let dest = word_to_usize(&self.stack.pop()?);
                let cond = self.stack.pop()?;
                if !is_zero(&cond) {
                    if dest >= self.code.len() || self.code[dest] != 0x5b {
                        return Err(ExecutionError::InvalidJump);
                    }
                    self.pc = dest;
                }
            }
            
            Opcode::JumpDest => {
                // NOP
            }
            
            Opcode::PC => {
                let mut pc_word = [0u8; 32];
                let pc_bytes = (self.pc as u64).to_be_bytes();
                pc_word[24..].copy_from_slice(&pc_bytes);
                self.stack.push(pc_word)?;
            }
            
            Opcode::Gas => {
                let mut gas_word = [0u8; 32];
                let gas_bytes = self.gas_meter.remaining().to_be_bytes();
                gas_word[24..].copy_from_slice(&gas_bytes);
                self.stack.push(gas_word)?;
            }
            
            Opcode::Sha3 => {
                let offset = word_to_usize(&self.stack.pop()?);
                let size = word_to_usize(&self.stack.pop()?);
                let data = self.memory.read(offset, size);
                
                let mut hasher = Keccak256::new();
                hasher.update(&data);
                let hash: [u8; 32] = hasher.finalize().into();
                
                self.stack.push(hash)?;
            }
            
            Opcode::Return => {
                let offset = word_to_usize(&self.stack.pop()?);
                let size = word_to_usize(&self.stack.pop()?);
                self.return_data = self.memory.read(offset, size);
                self.stopped = true;
            }
            
            Opcode::Revert => {
                let offset = word_to_usize(&self.stack.pop()?);
                let size = word_to_usize(&self.stack.pop()?);
                let reason = self.memory.read(offset, size);
                return Err(ExecutionError::Revert(String::from_utf8_lossy(&reason).to_string()));
            }
            
            Opcode::Invalid => {
                return Err(ExecutionError::InvalidOpcode(0xfe));
            }
            
            _ => {
                // Basitleştirilmiş - diğer opcode'lar implemente edilmeli
                tracing::debug!("Unimplemented opcode: {:?}", opcode);
            }
        }
        
        Ok(())
    }
    
    pub fn gas_used(&self) -> u64 {
        self.gas_meter.used()
    }
    
    pub fn gas_remaining(&self) -> u64 {
        self.gas_meter.remaining()
    }
}

// ===================================================================
// WORD YARDIMCILARI
// ===================================================================

fn word_zero() -> [u8; 32] {
    [0u8; 32]
}

fn word_one() -> [u8; 32] {
    let mut word = [0u8; 32];
    word[31] = 1;
    word
}

fn word_to_u256(word: &[u8; 32]) -> [u64; 4] {
    [
        u64::from_be_bytes(word[0..8].try_into().unwrap()),
        u64::from_be_bytes(word[8..16].try_into().unwrap()),
        u64::from_be_bytes(word[16..24].try_into().unwrap()),
        u64::from_be_bytes(word[24..32].try_into().unwrap()),
    ]
}

fn u256_to_word(value: &[u64; 4]) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[0..8].copy_from_slice(&value[0].to_be_bytes());
    word[8..16].copy_from_slice(&value[1].to_be_bytes());
    word[16..24].copy_from_slice(&value[2].to_be_bytes());
    word[24..32].copy_from_slice(&value[3].to_be_bytes());
    word
}

fn word_to_usize(word: &[u8; 32]) -> usize {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&word[24..32]);
    u64::from_be_bytes(bytes) as usize
}

fn is_zero(word: &[u8; 32]) -> bool {
    word.iter().all(|&b| b == 0)
}

fn add(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let a_val = word_to_u256(a);
    let b_val = word_to_u256(b);
    
    let mut result = [0u64; 4];
    let mut carry = 0u64;
    
    for i in (0..4).rev() {
        let sum = a_val[i] as u128 + b_val[i] as u128 + carry as u128;
        result[i] = sum as u64;
        carry = (sum >> 64) as u64;
    }
    
    u256_to_word(&result)
}

fn mul(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let a_val = word_to_u256(a);
    let b_val = word_to_u256(b);
    
    let a_u256 = a_val[2] as u128 | ((a_val[3] as u128) << 64);
    let b_u256 = b_val[2] as u128 | ((b_val[3] as u128) << 64);
    
    let product = a_u256 * b_u256;
    
    let mut result = [0u64; 4];
    result[2] = product as u64;
    result[3] = (product >> 64) as u64;
    
    u256_to_word(&result)
}

fn sub(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let a_val = word_to_u256(a);
    let b_val = word_to_u256(b);
    
    let mut result = [0u64; 4];
    let mut borrow = 0u64;
    
    for i in (0..4).rev() {
        let a_with_borrow = a_val[i] as i128 - borrow as i128;
        let b_val_i = b_val[i] as i128;
        
        if a_with_borrow >= b_val_i {
            result[i] = (a_with_borrow - b_val_i) as u64;
            borrow = 0;
        } else {
            result[i] = ((1u128 << 64) as i128 + a_with_borrow - b_val_i) as u64;
            borrow = 1;
        }
    }
    
    u256_to_word(&result)
}

fn div(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    if is_zero(b) {
        return word_zero();
    }
    
    let a_val = word_to_u256(a);
    let b_val = word_to_u256(b);
    
    let a_u256 = a_val[2] as u128 | ((a_val[3] as u128) << 64);
    let b_u256 = b_val[2] as u128 | ((b_val[3] as u128) << 64);
    
    if b_u256 == 0 {
        return word_zero();
    }
    
    let quotient = a_u256 / b_u256;
    
    let mut result = [0u64; 4];
    result[2] = quotient as u64;
    result[3] = (quotient >> 64) as u64;
    
    u256_to_word(&result)
}

fn lt(a: &[u8; 32], b: &[u8; 32]) -> bool {
    for i in 0..32 {
        if a[i] < b[i] {
            return true;
        } else if a[i] > b[i] {
            return false;
        }
    }
    false
}

fn gt(a: &[u8; 32], b: &[u8; 32]) -> bool {
    for i in 0..32 {
        if a[i] > b[i] {
            return true;
        } else if a[i] < b[i] {
            return false;
        }
    }
    false
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_operations() {
        let mut stack = Stack::new();
        
        stack.push([1u8; 32]).unwrap();
        stack.push([2u8; 32]).unwrap();
        
        assert_eq!(stack.len(), 2);
        
        let val = stack.pop().unwrap();
        assert_eq!(val, [2u8; 32]);
        
        assert_eq!(stack.len(), 1);
    }
    
    #[test]
    fn test_memory_operations() {
        let mut mem = Memory::new();
        
        mem.write(0, &[1, 2, 3, 4]);
        assert_eq!(mem.read(0, 4), vec![1, 2, 3, 4]);
        
        mem.write_word(10, &[5u8; 32]);
        assert_eq!(mem.read_word(10), [5u8; 32]);
    }
    
    #[test]
    fn test_vm_simple() {
        let code = vec![
            0x60, 0x01, // PUSH1 1
            0x60, 0x02, // PUSH1 2
            0x01,       // ADD
            0x00,       // STOP
        ];
        
        let mut vm = VM::new(code, 100_000);
        let result = vm.run();
        
        assert!(result.is_ok());
        assert_eq!(vm.stack.len(), 1);
        
        let top = vm.stack.pop().unwrap();
        assert_eq!(top[31], 3); // 1 + 2 = 3
    }
    
    #[test]
    fn test_arithmetic() {
        let a = [0u8; 32];
        let mut b = [0u8; 32];
        b[31] = 5;
        let mut c = [0u8; 32];
        c[31] = 3;
        
        let sum = add(&b, &c);
        assert_eq!(sum[31], 8);
        
        let diff = sub(&b, &c);
        assert_eq!(diff[31], 2);
        
        let prod = mul(&b, &c);
        assert_eq!(prod[31], 15);
        
        let quot = div(&b, &c);
        assert_eq!(quot[31], 1);
    }
}