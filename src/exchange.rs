//! Exact, replayable multi-asset accounting for GRID Exchange.
//!
//! Native assets remain on their own chains. This state machine records GRID
//! Exchange liabilities in exact whole units and is committed by Genesis blocks.
//! Native GRID amounts are denominated in Chips.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::crypto::blake3_hex;
use crate::passkey::verify_operator_sig;

const EXCHANGE_FILE: &str = "exchange-state.json";
const SIGNING_DOMAIN: &str = "GRID-EXCHANGE-TRANSITION-v1";
pub const MAX_EXCHANGE_ENVELOPE_LIFETIME_SECS: i64 = 10 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeBalance {
    pub available_atomic: String,
    pub held_atomic: String,
}

impl Default for ExchangeBalance {
    fn default() -> Self {
        Self {
            available_atomic: "0".into(),
            held_atomic: "0".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeOrder {
    pub id: String,
    pub owner: String,
    pub market: String,
    pub side: String,
    pub price_atomic: String,
    pub original_atomic: String,
    pub remaining_atomic: String,
    pub hold_asset: String,
    pub held_atomic: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ExchangeTransition {
    DepositCredit {
        event_id: String,
        account_id: String,
        asset: String,
        amount_atomic: String,
        native_tx_id: String,
    },
    OrderHold {
        order_id: String,
        account_id: String,
        market: String,
        side: String,
        price_atomic: String,
        quantity_atomic: String,
        hold_asset: String,
        hold_atomic: String,
    },
    OrderRelease {
        order_id: String,
    },
    TradeFill {
        trade_id: String,
        market: String,
        maker_order_id: String,
        taker_order_id: String,
        price_atomic: String,
        quantity_atomic: String,
        maker_receive_atomic: String,
        taker_receive_atomic: String,
        maker_fee_asset: String,
        maker_fee_atomic: String,
        taker_fee_asset: String,
        taker_fee_atomic: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeIntent {
    pub version: u32,
    pub chain_id: String,
    pub transition_id: String,
    pub sequence: u64,
    pub actor: String,
    pub actor_nonce: u64,
    pub expires_at: i64,
    pub transition: ExchangeTransition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SignedExchangeIntent {
    pub intent: ExchangeIntent,
    pub public_key: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeStateV1 {
    pub version: u32,
    pub sequence: u64,
    #[serde(default)]
    pub actor_nonces: BTreeMap<String, u64>,
    #[serde(default)]
    pub balances: BTreeMap<String, BTreeMap<String, ExchangeBalance>>,
    #[serde(default)]
    pub processed_external_events: BTreeMap<String, String>,
    #[serde(default)]
    pub processed_transitions: BTreeSet<String>,
    #[serde(default)]
    pub orders: BTreeMap<String, ExchangeOrder>,
    #[serde(default)]
    pub fees: BTreeMap<String, String>,
}

impl Default for ExchangeStateV1 {
    fn default() -> Self {
        Self {
            version: 1,
            sequence: 0,
            actor_nonces: BTreeMap::new(),
            balances: BTreeMap::new(),
            processed_external_events: BTreeMap::new(),
            processed_transitions: BTreeSet::new(),
            orders: BTreeMap::new(),
            fees: BTreeMap::new(),
        }
    }
}

pub fn exchange_signing_bytes(intent: &ExchangeIntent) -> Result<Vec<u8>> {
    let mut bytes = SIGNING_DOMAIN.as_bytes().to_vec();
    bytes.push(b'\n');
    bytes.extend(serde_json::to_vec(intent)?);
    Ok(bytes)
}

pub fn validate_signed_exchange_intent(
    envelope: &SignedExchangeIntent,
    expected_chain_id: &str,
) -> Result<()> {
    validate_exchange_signature(envelope, expected_chain_id)?;
    let intent = &envelope.intent;
    let now = Utc::now().timestamp();
    if intent.expires_at < now {
        bail!("exchange transition expired");
    }
    if intent.expires_at > now + MAX_EXCHANGE_ENVELOPE_LIFETIME_SECS {
        bail!("exchange transition expiry is too far in the future");
    }
    Ok(())
}

fn validate_exchange_signature(
    envelope: &SignedExchangeIntent,
    expected_chain_id: &str,
) -> Result<()> {
    let intent = &envelope.intent;
    if intent.version != 1 {
        bail!("unsupported exchange transition version");
    }
    if intent.chain_id != expected_chain_id {
        bail!("exchange transition is for a different chain");
    }
    if envelope.public_key != intent.actor {
        bail!("exchange actor does not match signing key");
    }
    if intent.transition_id.is_empty() || intent.transition_id.len() > 128 {
        bail!("invalid transition id");
    }
    verify_operator_sig(
        &envelope.public_key,
        &exchange_signing_bytes(intent)?,
        &envelope.signature,
    )
}

impl ExchangeStateV1 {
    pub fn path_in(config_dir: &Path) -> PathBuf {
        config_dir.join(EXCHANGE_FILE)
    }

    pub fn load(config_dir: &Path) -> Result<Self> {
        let path = Self::path_in(config_dir);
        if !path.exists() {
            return Ok(Self::default());
        }
        serde_json::from_slice(&std::fs::read(&path)?).context("decode exchange state")
    }

    pub fn save(&self, config_dir: &Path) -> Result<()> {
        let path = Self::path_in(config_dir);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn state_root(&self) -> Result<String> {
        let mut bytes = b"GRID-EXCHANGE-STATE-v1\n".to_vec();
        bytes.extend(serde_json::to_vec(self)?);
        Ok(blake3_hex(&bytes))
    }

    pub fn apply_signed(&mut self, envelope: &SignedExchangeIntent, chain_id: &str) -> Result<()> {
        validate_signed_exchange_intent(envelope, chain_id)?;
        self.apply_validated(envelope)
    }

    /// Replay a leader-committed transition without evaluating expiry against
    /// wall-clock time. Signature, chain, sequence, nonce, and state invariants
    /// remain fully verified.
    pub fn apply_committed(
        &mut self,
        envelope: &SignedExchangeIntent,
        chain_id: &str,
    ) -> Result<()> {
        validate_exchange_signature(envelope, chain_id)?;
        self.apply_validated(envelope)
    }

    fn apply_validated(&mut self, envelope: &SignedExchangeIntent) -> Result<()> {
        let intent = &envelope.intent;
        if self.processed_transitions.contains(&intent.transition_id) {
            bail!("duplicate exchange transition");
        }
        let expected_sequence = self.sequence.saturating_add(1);
        if intent.sequence != expected_sequence {
            bail!("invalid exchange sequence: expected {expected_sequence}");
        }
        let expected_nonce = self
            .actor_nonces
            .get(&intent.actor)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        if intent.actor_nonce != expected_nonce {
            bail!("invalid exchange actor nonce: expected {expected_nonce}");
        }

        match &intent.transition {
            ExchangeTransition::DepositCredit {
                event_id,
                account_id,
                asset,
                amount_atomic,
                native_tx_id,
            } => {
                validate_identifier(event_id, "event id")?;
                validate_identifier(native_tx_id, "native transaction id")?;
                validate_asset(asset)?;
                let amount = atomic(amount_atomic)?;
                if self.processed_external_events.contains_key(event_id) {
                    bail!("external deposit event already processed");
                }
                self.credit_available(account_id, asset, amount)?;
                self.processed_external_events
                    .insert(event_id.clone(), intent.transition_id.clone());
            }
            ExchangeTransition::OrderHold {
                order_id,
                account_id,
                market,
                side,
                price_atomic,
                quantity_atomic,
                hold_asset,
                hold_atomic,
            } => {
                validate_identifier(order_id, "order id")?;
                validate_market(market)?;
                if side != "buy" && side != "sell" {
                    bail!("invalid order side");
                }
                atomic(price_atomic)?;
                atomic(quantity_atomic)?;
                let hold = atomic(hold_atomic)?;
                validate_asset(hold_asset)?;
                if self.orders.contains_key(order_id) {
                    bail!("duplicate order id");
                }
                self.move_available_to_held(account_id, hold_asset, hold)?;
                self.orders.insert(
                    order_id.clone(),
                    ExchangeOrder {
                        id: order_id.clone(),
                        owner: account_id.clone(),
                        market: market.clone(),
                        side: side.clone(),
                        price_atomic: price_atomic.clone(),
                        original_atomic: quantity_atomic.clone(),
                        remaining_atomic: quantity_atomic.clone(),
                        hold_asset: hold_asset.clone(),
                        held_atomic: hold_atomic.clone(),
                        status: "open".into(),
                    },
                );
            }
            ExchangeTransition::OrderRelease { order_id } => {
                let order = self
                    .orders
                    .get(order_id)
                    .cloned()
                    .context("order not found")?;
                if order.status != "open" {
                    bail!("order is not open");
                }
                let held = atomic(&order.held_atomic)?;
                self.move_held_to_available(&order.owner, &order.hold_asset, held)?;
                let stored = self.orders.get_mut(order_id).expect("order exists");
                stored.held_atomic = "0".into();
                stored.status = "cancelled".into();
            }
            ExchangeTransition::TradeFill {
                trade_id,
                market,
                maker_order_id,
                taker_order_id,
                price_atomic,
                quantity_atomic,
                maker_receive_atomic,
                taker_receive_atomic,
                maker_fee_asset,
                maker_fee_atomic,
                taker_fee_asset,
                taker_fee_atomic,
            } => {
                validate_identifier(trade_id, "trade id")?;
                validate_market(market)?;
                let price = atomic(price_atomic)?;
                let quantity = atomic(quantity_atomic)?;
                let maker_receive = atomic_allow_zero(maker_receive_atomic)?;
                let taker_receive = atomic_allow_zero(taker_receive_atomic)?;
                let maker_fee = atomic_allow_zero(maker_fee_atomic)?;
                let taker_fee = atomic_allow_zero(taker_fee_atomic)?;
                validate_asset(maker_fee_asset)?;
                validate_asset(taker_fee_asset)?;
                self.apply_fill(
                    maker_order_id,
                    taker_order_id,
                    market,
                    price,
                    quantity,
                    maker_receive,
                    taker_receive,
                    maker_fee_asset,
                    maker_fee,
                    taker_fee_asset,
                    taker_fee,
                )?;
            }
        }

        self.sequence = intent.sequence;
        self.actor_nonces
            .insert(intent.actor.clone(), intent.actor_nonce);
        self.processed_transitions
            .insert(intent.transition_id.clone());
        Ok(())
    }

    fn apply_fill(
        &mut self,
        maker_id: &str,
        taker_id: &str,
        market: &str,
        _price: u128,
        quantity: u128,
        maker_receive: u128,
        taker_receive: u128,
        maker_fee_asset: &str,
        maker_fee: u128,
        taker_fee_asset: &str,
        taker_fee: u128,
    ) -> Result<()> {
        let maker = self
            .orders
            .get(maker_id)
            .cloned()
            .context("maker order not found")?;
        let taker = self
            .orders
            .get(taker_id)
            .cloned()
            .context("taker order not found")?;
        if maker.status != "open" || taker.status != "open" {
            bail!("both orders must be open");
        }
        if maker.market != market || taker.market != market || maker.side == taker.side {
            bail!("orders do not form a market match");
        }
        if maker.owner == taker.owner {
            bail!("self trade is forbidden");
        }
        if maker_fee_asset != maker.hold_asset || taker_fee_asset != taker.hold_asset {
            bail!("fees must be denominated in each order's held asset");
        }
        let maker_remaining = atomic(&maker.remaining_atomic)?;
        let taker_remaining = atomic(&taker.remaining_atomic)?;
        if quantity > maker_remaining || quantity > taker_remaining {
            bail!("fill exceeds remaining quantity");
        }

        let maker_held = atomic(&maker.held_atomic)?;
        let taker_held = atomic(&taker.held_atomic)?;
        let maker_spend = taker_receive
            .checked_add(maker_fee)
            .context("maker spend overflow")?;
        let taker_spend = maker_receive
            .checked_add(taker_fee)
            .context("taker spend overflow")?;
        if maker_spend > maker_held || taker_spend > taker_held {
            bail!("fill exceeds committed hold");
        }

        self.debit_held(&maker.owner, &maker.hold_asset, maker_spend)?;
        self.debit_held(&taker.owner, &taker.hold_asset, taker_spend)?;
        let (base, quote) = split_market(market)?;
        let maker_receive_asset = if maker.side == "sell" { quote } else { base };
        let taker_receive_asset = if taker.side == "sell" { quote } else { base };
        self.credit_available(&maker.owner, maker_receive_asset, maker_receive)?;
        self.credit_available(&taker.owner, taker_receive_asset, taker_receive)?;
        self.credit_fee(maker_fee_asset, maker_fee)?;
        self.credit_fee(taker_fee_asset, taker_fee)?;

        self.update_filled_order_and_release(maker_id, quantity, maker_spend)?;
        self.update_filled_order_and_release(taker_id, quantity, taker_spend)?;
        Ok(())
    }

    fn update_filled_order_and_release(
        &mut self,
        order_id: &str,
        quantity: u128,
        spend: u128,
    ) -> Result<()> {
        let release = {
            let order = self.orders.get_mut(order_id).context("order not found")?;
            update_filled_order(order, quantity, spend)?;
            if order.status == "filled" {
                let residual = atomic_allow_zero(&order.held_atomic)?;
                order.held_atomic = "0".into();
                Some((order.owner.clone(), order.hold_asset.clone(), residual))
            } else {
                None
            }
        };
        if let Some((owner, asset, residual)) = release {
            if residual > 0 {
                self.move_held_to_available(&owner, &asset, residual)?;
            }
        }
        Ok(())
    }

    fn balance_mut(&mut self, account: &str, asset: &str) -> Result<&mut ExchangeBalance> {
        validate_identifier(account, "account id")?;
        validate_asset(asset)?;
        Ok(self
            .balances
            .entry(account.into())
            .or_default()
            .entry(asset.into())
            .or_default())
    }

    fn credit_available(&mut self, account: &str, asset: &str, amount: u128) -> Result<()> {
        let balance = self.balance_mut(account, asset)?;
        balance.available_atomic = add_atomic(&balance.available_atomic, amount)?.to_string();
        Ok(())
    }

    fn move_available_to_held(&mut self, account: &str, asset: &str, amount: u128) -> Result<()> {
        let balance = self.balance_mut(account, asset)?;
        let available = atomic_allow_zero(&balance.available_atomic)?;
        if amount > available {
            bail!("insufficient available balance");
        }
        balance.available_atomic = (available - amount).to_string();
        balance.held_atomic = add_atomic(&balance.held_atomic, amount)?.to_string();
        Ok(())
    }

    fn move_held_to_available(&mut self, account: &str, asset: &str, amount: u128) -> Result<()> {
        self.debit_held(account, asset, amount)?;
        self.credit_available(account, asset, amount)
    }

    fn debit_held(&mut self, account: &str, asset: &str, amount: u128) -> Result<()> {
        let balance = self.balance_mut(account, asset)?;
        let held = atomic_allow_zero(&balance.held_atomic)?;
        if amount > held {
            bail!("insufficient held balance");
        }
        balance.held_atomic = (held - amount).to_string();
        Ok(())
    }

    fn credit_fee(&mut self, asset: &str, amount: u128) -> Result<()> {
        let current = self.fees.get(asset).map(String::as_str).unwrap_or("0");
        self.fees
            .insert(asset.into(), add_atomic(current, amount)?.to_string());
        Ok(())
    }
}

fn update_filled_order(order: &mut ExchangeOrder, quantity: u128, spend: u128) -> Result<()> {
    let remaining = atomic(&order.remaining_atomic)?;
    let held = atomic_allow_zero(&order.held_atomic)?;
    order.remaining_atomic = (remaining - quantity).to_string();
    order.held_atomic = (held - spend).to_string();
    if remaining == quantity {
        order.status = "filled".into();
    }
    Ok(())
}

fn validate_identifier(value: &str, field: &str) -> Result<()> {
    if value.is_empty() || value.len() > 160 || !value.bytes().all(|b| b.is_ascii_graphic()) {
        bail!("invalid {field}");
    }
    Ok(())
}

fn validate_asset(asset: &str) -> Result<()> {
    if !matches!(asset, "GRID" | "BTC" | "ETH" | "SOL" | "USDC") {
        bail!("unsupported exchange asset");
    }
    Ok(())
}

fn validate_market(market: &str) -> Result<()> {
    let (base, quote) = split_market(market)?;
    validate_asset(base)?;
    validate_asset(quote)?;
    if base == quote {
        bail!("invalid exchange market");
    }
    Ok(())
}

fn split_market(market: &str) -> Result<(&str, &str)> {
    market.split_once('-').context("invalid exchange market")
}

fn atomic(value: &str) -> Result<u128> {
    let parsed = atomic_allow_zero(value)?;
    if parsed == 0 {
        bail!("atomic amount must be positive");
    }
    Ok(parsed)
}

fn atomic_allow_zero(value: &str) -> Result<u128> {
    if value.is_empty() || (value.len() > 1 && value.starts_with('0')) {
        bail!("atomic amount is not canonical");
    }
    value.parse::<u128>().context("invalid atomic amount")
}

fn add_atomic(current: &str, amount: u128) -> Result<u128> {
    atomic_allow_zero(current)?
        .checked_add(amount)
        .context("atomic amount overflow")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed(
        secret: &SigningKey,
        sequence: u64,
        nonce: u64,
        transition: ExchangeTransition,
    ) -> SignedExchangeIntent {
        let public_key = hex::encode(secret.verifying_key().as_bytes());
        let intent = ExchangeIntent {
            version: 1,
            chain_id: "grid-test".into(),
            transition_id: format!("transition-{sequence}"),
            sequence,
            actor: public_key.clone(),
            actor_nonce: nonce,
            expires_at: Utc::now().timestamp() + 120,
            transition,
        };
        let signature = hex::encode(
            secret
                .sign(&exchange_signing_bytes(&intent).unwrap())
                .to_bytes(),
        );
        SignedExchangeIntent {
            intent,
            public_key,
            signature,
        }
    }

    #[test]
    fn deposit_is_exact_and_idempotent() {
        let secret = SigningKey::from_bytes(&[21u8; 32]);
        let mut state = ExchangeStateV1::default();
        let deposit = signed(
            &secret,
            1,
            1,
            ExchangeTransition::DepositCredit {
                event_id: "btc:tx:0".into(),
                account_id: "user-1".into(),
                asset: "BTC".into(),
                amount_atomic: "12345678".into(),
                native_tx_id: "tx".into(),
            },
        );
        state.apply_signed(&deposit, "grid-test").unwrap();
        assert_eq!(state.balances["user-1"]["BTC"].available_atomic, "12345678");
        assert!(state.apply_signed(&deposit, "grid-test").is_err());
    }

    #[test]
    fn hold_and_release_never_create_value() {
        let secret = SigningKey::from_bytes(&[22u8; 32]);
        let mut state = ExchangeStateV1::default();
        state
            .apply_signed(
                &signed(
                    &secret,
                    1,
                    1,
                    ExchangeTransition::DepositCredit {
                        event_id: "usdc:tx:0".into(),
                        account_id: "buyer".into(),
                        asset: "USDC".into(),
                        amount_atomic: "1000000".into(),
                        native_tx_id: "tx".into(),
                    },
                ),
                "grid-test",
            )
            .unwrap();
        state
            .apply_signed(
                &signed(
                    &secret,
                    2,
                    2,
                    ExchangeTransition::OrderHold {
                        order_id: "order-1".into(),
                        account_id: "buyer".into(),
                        market: "BTC-USDC".into(),
                        side: "buy".into(),
                        price_atomic: "60000000000".into(),
                        quantity_atomic: "1000".into(),
                        hold_asset: "USDC".into(),
                        hold_atomic: "700000".into(),
                    },
                ),
                "grid-test",
            )
            .unwrap();
        state
            .apply_signed(
                &signed(
                    &secret,
                    3,
                    3,
                    ExchangeTransition::OrderRelease {
                        order_id: "order-1".into(),
                    },
                ),
                "grid-test",
            )
            .unwrap();
        let balance = &state.balances["buyer"]["USDC"];
        assert_eq!(balance.available_atomic, "1000000");
        assert_eq!(balance.held_atomic, "0");
    }

    #[test]
    fn grid_usdc_fill_conserves_chips_and_releases_price_improvement() {
        let secret = SigningKey::from_bytes(&[23u8; 32]);
        let mut state = ExchangeStateV1::default();
        let transitions = [
            ExchangeTransition::DepositCredit {
                event_id: "grid:deposit:seller".into(),
                account_id: "seller".into(),
                asset: "GRID".into(),
                amount_atomic: "102".into(),
                native_tx_id: "grid-native-1".into(),
            },
            ExchangeTransition::DepositCredit {
                event_id: "usdc:deposit:buyer".into(),
                account_id: "buyer".into(),
                asset: "USDC".into(),
                amount_atomic: "220".into(),
                native_tx_id: "usdc-native-1".into(),
            },
            ExchangeTransition::OrderHold {
                order_id: "sell-grid".into(),
                account_id: "seller".into(),
                market: "GRID-USDC".into(),
                side: "sell".into(),
                price_atomic: "2".into(),
                quantity_atomic: "100".into(),
                hold_asset: "GRID".into(),
                hold_atomic: "102".into(),
            },
            ExchangeTransition::OrderHold {
                order_id: "buy-grid".into(),
                account_id: "buyer".into(),
                market: "GRID-USDC".into(),
                side: "buy".into(),
                price_atomic: "2".into(),
                quantity_atomic: "100".into(),
                hold_asset: "USDC".into(),
                hold_atomic: "220".into(),
            },
            ExchangeTransition::TradeFill {
                trade_id: "grid-usdc-trade-1".into(),
                market: "GRID-USDC".into(),
                maker_order_id: "sell-grid".into(),
                taker_order_id: "buy-grid".into(),
                price_atomic: "2".into(),
                quantity_atomic: "100".into(),
                maker_receive_atomic: "198".into(),
                taker_receive_atomic: "100".into(),
                maker_fee_asset: "GRID".into(),
                maker_fee_atomic: "2".into(),
                taker_fee_asset: "USDC".into(),
                taker_fee_atomic: "2".into(),
            },
        ];

        for (index, transition) in transitions.into_iter().enumerate() {
            let sequence = index as u64 + 1;
            state
                .apply_signed(
                    &signed(&secret, sequence, sequence, transition),
                    "grid-test",
                )
                .unwrap();
        }

        assert_eq!(state.balances["buyer"]["GRID"].available_atomic, "100");
        assert_eq!(state.balances["seller"]["USDC"].available_atomic, "198");
        assert_eq!(state.balances["buyer"]["USDC"].available_atomic, "20");
        assert_eq!(state.balances["buyer"]["USDC"].held_atomic, "0");
        assert_eq!(state.fees["GRID"], "2");
        assert_eq!(state.fees["USDC"], "2");
        assert_eq!(state.orders["sell-grid"].status, "filled");
        assert_eq!(state.orders["buy-grid"].status, "filled");
    }
}
