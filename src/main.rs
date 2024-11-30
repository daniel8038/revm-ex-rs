use anyhow::{Ok, Result};
use bytes::Bytes;
use ethers_contract::BaseContract;
use ethers_core::abi::parse_abi;
use ethers_providers::{Http, Provider};
use revm::{
    db::{CacheDB, EmptyDB, EthersDB},
    primitives::{ExecutionResult, Output, TransactTo, U256 as rU256},
    Database, Evm,
};
use revm_primitives::{Address, TxEnv};
use std::{env, str::FromStr, sync::Arc};

use dotenv::dotenv;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let https_url = env::var("HTTP_URL").unwrap();
    let client = Provider::<Http>::try_from(https_url)?;
    let client = Arc::new(client);
    /////////////////////////////
    ////轻量级"的主网分叉/////////
    ////////////////////////////
    // CacheDB  一个带缓存的数据库实现，可以缓存账户状态和存储数据
    // EmptyDB  空数据库实现，用于测试或不需要状态的场景
    // EthersDB 与 ethers-rs 库集成的数据库实现，可以从以太坊节点获取数据
    // 创建EthersDB类型数据库
    let mut ethersdb = EthersDB::new(client.clone(), None).unwrap();
    // 将 Uniswap V2 中的 WETH-USDT 池地址设置为pool_address
    let pool_address = Address::from_str("0x0d4a11d5EEaaC28EC3F61d100daF4d40471f1852")?;
    // 使用ethersdb调用函数“ basic ” 。这将通过异步请求每个节点提供商来检索nonce、余额和代码数据，
    // 返回类型AccountInfo balance: U256 nonce: u64 code_hash: B256 code: Option<Bytecode>
    let acc_info = ethersdb.basic(pool_address).unwrap().unwrap();
    // 8 是 UniswapV2Pair 智能合约中定义的(reserve0, reserve1, blockTimestampLast)变量的存储槽索引。
    let slot = rU256::from(8);
    // value 是一个 U256（256位整数），它包含了池子的储备量信息
    // 在 Uniswap V2 中，槽位 8 存储了三个打包在一起的值 reserve0 reserve1 blockTimestampLast: 最后更新时间
    // let reserve0 = (value >> 112) & ((1u128 << 112) - 1).into();  // 提取WETH储备量
    // let reserve1 = (value >> 0) & ((1u128 << 112) - 1).into();    // 提取USDT储备量
    let value = ethersdb.storage(pool_address, slot).unwrap();
    println!("{:?}", value); // 0x64ca691b00000000000000001d11899c51780000000003aa5712d4e77e453b6c_U256
                             // 创建一个空的缓存数据库
    let mut cache_db = CacheDB::new(EmptyDB::default());
    // 将合约的基本信息（代码、nonce等）插入缓存
    cache_db.insert_account_info(pool_address, acc_info);
    // 将合约的储备量信息插入缓存
    cache_db.insert_account_storage(pool_address, slot, value);
    let pool_contract = BaseContract::from(parse_abi(&["function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast)"])?);
    let encoded = pool_contract.encode("getReserves", ())?;
    let caller = Address::from_str("0x0000000000000000000000000000000000000000")?;
    // 使用构建器的默认配置
    let mut evm = Evm::builder()
        .with_db(cache_db)
        .with_tx_env(TxEnv {
            caller,
            transact_to: TransactTo::Call(pool_address),
            data: encoded.0.into(),
            value: rU256::ZERO,
            ..Default::default()
        })
        .build();
    let ref_tx = evm.transact().unwrap();
    let result = ref_tx.result;
    let value = match result {
        ExecutionResult::Success { output, .. } => match output {
            Output::Call(value) => Some(value),
            _ => None,
        },
        _ => None,
    };
    println!("value====>{:?}", value);
    // reserve0 reserve1 blockTimestampLast: 最后更新时间
    let (reserve0, reserve1, ts): (u128, u128, u32) =
        pool_contract.decode_output("getReserves", value.unwrap())?;
    // 我们确认“ getReserves ”函数调用返回了我们注入到CacheDB的储备值。
    println!(
        "pool_contract.decode_output>>>>>>>>>{:?} {:?} {:?}",
        reserve0, reserve1, ts
    );
    Ok(())
}
// EthersDB:
// 它不是一个真正的数据库，而是一个数据访问接口
// 每次调用都会实时从以太坊节点（通过你提供的 RPC URL）获取数据
// CacheDB:
// 它是一个纯内存的缓存层，数据存在 RAM 中，不需要启动任何数据库服务
// 主要目的是：
// 避免重复的网络请求（性能优化） 如果EVM需要读取这些数据，直接从内存获取，不会再发起网络请求
// 允许本地修改状态（而不影响主网）
// 提供一致的状态视图（同一个交易模拟过程中状态保持一致）
