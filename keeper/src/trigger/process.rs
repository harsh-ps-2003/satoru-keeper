use std::sync::Arc;

use sqlx::PgPool;
use starknet::{
    accounts::{ConnectedAccount, SingleOwnerAccount},
    core::types::{BlockId, BlockTag, MaybePendingBlockWithTxHashes},
    providers::{jsonrpc::HttpTransport, JsonRpcClient, Provider},
    signers::LocalWallet,
};

use crate::{
    liquidation::utils::{MarketPrices, Order, OrderType},
    price::utils::get_market_prices,
    query::get_market,
};

pub async fn get_triggerable_orders(
    pool: &PgPool,
    account: Arc<SingleOwnerAccount<JsonRpcClient<HttpTransport>, LocalWallet>>,
) -> Result<Vec<Order>, sqlx::Error> {
    let orders: Vec<Order> = sqlx::query_as("SELECT * FROM orders")
        .fetch_all(pool)
        .await?;

    let block = Arc::clone(&account)
        .provider()
        .get_block_with_tx_hashes(BlockId::Tag(BlockTag::Latest))
        .await
        .expect("Could not fetch latest block");
    let timestamp = match block {
        MaybePendingBlockWithTxHashes::Block(block) => block.timestamp,
        MaybePendingBlockWithTxHashes::PendingBlock(block) => block.timestamp,
    };

    let mut triggerable_positions: Vec<Order> = Vec::new();
    for order in orders {
        if is_limit_order(order.clone().order_type) {
            let market = get_market(order.clone().market.0.to_string(), pool)
                .await
                .expect("Could not get market");
            let market_prices: MarketPrices =
                get_market_prices(market.clone(), timestamp.to_string())
                    .await
                    .expect(
                        format!("Could not get market prices for market: :{:?}", market).as_str(),
                    );
            if should_trigger(order.clone(), market_prices) {
                triggerable_positions.push(order.clone());
                log::info!("Position {:?} is liquidatable", order.clone().key);
            }
        }
    }

    Ok(triggerable_positions)
}

pub fn is_limit_order(order_type: OrderType) -> bool {
    match order_type {
        OrderType::LimitSwap => true,
        OrderType::LimitIncrease => true,
        OrderType::LimitDecrease => true,
        OrderType::StopLossDecrease => true,
        _ => false,
    }
}

pub fn should_trigger(order: Order, market_prices: MarketPrices) -> bool {
    // TODO check when to use max or min
    // TODO depending on is_long
    match order.order_type {
        OrderType::LimitSwap => market_prices.index_token_price.max <= order.trigger_price,
        OrderType::LimitIncrease => market_prices.index_token_price.max <= order.trigger_price,
        OrderType::LimitDecrease => market_prices.index_token_price.max >= order.trigger_price,
        OrderType::StopLossDecrease => market_prices.index_token_price.max <= order.trigger_price,
        _ => false,
    }
}
