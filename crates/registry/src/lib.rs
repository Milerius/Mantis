//! Instrument registry with venue bindings for the Mantis SDK.
//!
//! Maps venue-specific identifiers (Polymarket `token_id`, Binance `symbol`)
//! to stable internal [`mantis_types::InstrumentId`] values with O(1) average
//! lookup time.
//!
//! The registry provides a stable identity layer that survives Polymarket's
//! recurring market rotation — the same `InstrumentId` persists across window
//! changes while the underlying `token_id` rotates every 5/15/60 minutes.

#![deny(unsafe_code)]

mod bindings;
mod error;
mod record;
mod types;

pub use bindings::{BinanceBinding, PolymarketBinding, PolymarketWindowBinding};
pub use error::RegistryError;
pub use record::{CanonicalInstrument, InstrumentRecord};
pub use types::{Asset, InstrumentClass, InstrumentKey, OutcomeSide, Timeframe};

use std::collections::HashMap;

use mantis_types::{InstrumentId, InstrumentMeta};

/// Read-optimized instrument registry with venue-specific reverse indexes.
///
/// # Hot boundary path (O(1), no allocation)
///
/// ```text
/// venue_id → InstrumentId       (one HashMap lookup)
/// InstrumentId → InstrumentMeta (one HashMap lookup)
/// ```
///
/// # Control path (infrequent, may allocate)
///
/// ```text
/// insert() — register new instruments at startup
/// bind_polymarket_current/next() — update rotating window bindings
/// promote_polymarket_next() — roll current → expired, next → current
/// ```
pub struct InstrumentRegistry<const D: u8> {
    by_id: HashMap<InstrumentId, InstrumentRecord<D>>,
    by_key: HashMap<InstrumentKey, InstrumentId>,
    by_binance_symbol: HashMap<String, InstrumentId>,
    by_polymarket_token_id: HashMap<String, InstrumentId>,
    next_id: u32,
}

impl<const D: u8> InstrumentRegistry<D> {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_id: HashMap::new(),
            by_key: HashMap::new(),
            by_binance_symbol: HashMap::new(),
            by_polymarket_token_id: HashMap::new(),
            next_id: 1, // 0 is reserved for InstrumentId::NONE
        }
    }

    // Read path (hot boundary)

    /// Look up a full instrument record by ID.
    #[must_use]
    pub fn get(&self, id: InstrumentId) -> Option<&InstrumentRecord<D>> {
        self.by_id.get(&id)
    }

    /// Look up instrument metadata by ID (for `price_to_ticks` / `qty_to_lots`).
    #[must_use]
    pub fn meta(&self, id: InstrumentId) -> Option<&InstrumentMeta<D>> {
        self.by_id.get(&id).map(|r| &r.canonical.meta)
    }

    /// Resolve a Binance symbol to an `InstrumentId`.
    #[must_use]
    pub fn by_binance_symbol(&self, symbol: &str) -> Option<InstrumentId> {
        self.by_binance_symbol.get(symbol).copied()
    }

    /// Resolve a Polymarket token ID to an `InstrumentId`.
    #[must_use]
    pub fn by_polymarket_token_id(&self, token_id: &str) -> Option<InstrumentId> {
        self.by_polymarket_token_id.get(token_id).copied()
    }

    /// Resolve an `InstrumentKey` to an `InstrumentId`.
    #[must_use]
    pub fn by_key(&self, key: &InstrumentKey) -> Option<InstrumentId> {
        self.by_key.get(key).copied()
    }

    /// All currently active Polymarket token IDs (for WS subscription).
    #[must_use]
    pub fn active_polymarket_token_ids(&self) -> Vec<String> {
        self.by_polymarket_token_id.keys().cloned().collect()
    }

    /// Number of registered instruments.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    // Mutation path (control, infrequent)

    /// Allocate the next `InstrumentId`. IDs are monotonic, never recycled.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::IdSpaceExhausted`] if all 2^32 - 1 IDs are used.
    fn alloc_id(&mut self) -> Result<InstrumentId, RegistryError> {
        if self.next_id == 0 {
            // Wrapped around — 0 is reserved for InstrumentId::NONE
            return Err(RegistryError::IdSpaceExhausted);
        }
        let id = InstrumentId::from_raw(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        Ok(id)
    }

    /// Register a new instrument with its canonical identity and metadata.
    ///
    /// Returns the assigned `InstrumentId`.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::DuplicateInstrumentKey`] if an instrument
    /// with the same key already exists.
    pub fn insert(
        &mut self,
        key: InstrumentKey,
        meta: InstrumentMeta<D>,
        binance: Option<BinanceBinding>,
        polymarket: Option<PolymarketBinding>,
    ) -> Result<InstrumentId, RegistryError> {
        if self.by_key.contains_key(&key) {
            return Err(RegistryError::DuplicateInstrumentKey);
        }

        let id = self.alloc_id()?;

        // Index Binance symbol
        if let Some(ref bn) = binance
            && self.by_binance_symbol.contains_key(&bn.symbol)
        {
            return Err(RegistryError::DuplicateBinanceSymbol(bn.symbol.clone()));
        }
        if let Some(ref bn) = binance {
            self.by_binance_symbol.insert(bn.symbol.clone(), id);
        }

        let record = InstrumentRecord {
            canonical: CanonicalInstrument {
                instrument_id: id,
                key: key.clone(),
                meta,
            },
            binance,
            polymarket,
        };

        self.by_key.insert(key, id);
        self.by_id.insert(id, record);

        Ok(id)
    }

    /// Register an Up/Down prediction pair in one call.
    ///
    /// Returns `(up_id, down_id)`.
    ///
    /// # Errors
    ///
    /// Returns a `RegistryError` if either key is a duplicate.
    pub fn insert_prediction_pair(
        &mut self,
        base: Asset,
        timeframe: Timeframe,
        meta: InstrumentMeta<D>,
        binance_symbol: Option<&str>,
    ) -> Result<(InstrumentId, InstrumentId), RegistryError> {
        let up_key = InstrumentKey::prediction(base, timeframe, OutcomeSide::Up);
        let down_key = InstrumentKey::prediction(base, timeframe, OutcomeSide::Down);

        // Pre-check both keys to avoid partial insert (Up succeeds, Down fails)
        if self.by_key.contains_key(&up_key) || self.by_key.contains_key(&down_key) {
            return Err(RegistryError::DuplicateInstrumentKey);
        }
        if let Some(sym) = binance_symbol
            && self.by_binance_symbol.contains_key(sym)
        {
            return Err(RegistryError::DuplicateBinanceSymbol(sym.to_owned()));
        }

        let bn_binding = binance_symbol.map(|s| BinanceBinding {
            symbol: s.to_owned(),
        });

        let up_id = self.insert(
            up_key,
            meta,
            bn_binding.clone(),
            Some(PolymarketBinding::default()),
        )?;
        let down_id = self.insert(
            down_key,
            meta,
            None, // Binance symbol shared via Up instrument
            Some(PolymarketBinding::default()),
        )?;

        Ok((up_id, down_id))
    }

    /// Bind a Polymarket window as the current active window for an instrument.
    ///
    /// Updates the reverse index: `token_id → InstrumentId`.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument doesn't exist or has no Polymarket binding.
    pub fn bind_polymarket_current(
        &mut self,
        id: InstrumentId,
        binding: PolymarketWindowBinding,
    ) -> Result<(), RegistryError> {
        let record = self
            .by_id
            .get_mut(&id)
            .ok_or(RegistryError::MissingInstrument(id))?;
        let pm = record
            .polymarket
            .as_mut()
            .ok_or(RegistryError::MissingPolymarketBinding(id))?;

        // Remove old token_id from reverse index
        if let Some(ref old) = pm.current {
            self.by_polymarket_token_id.remove(&old.token_id);
        }

        // Insert new token_id into reverse index
        self.by_polymarket_token_id
            .insert(binding.token_id.clone(), id);
        pm.current = Some(binding);

        Ok(())
    }

    /// Bind a Polymarket window as the next upcoming window (pre-subscribe).
    ///
    /// Adds the next window's `token_id` to the reverse index so the ingest
    /// layer can resolve it immediately when data arrives.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument doesn't exist or has no Polymarket binding.
    pub fn bind_polymarket_next(
        &mut self,
        id: InstrumentId,
        binding: PolymarketWindowBinding,
    ) -> Result<(), RegistryError> {
        let record = self
            .by_id
            .get_mut(&id)
            .ok_or(RegistryError::MissingInstrument(id))?;
        let pm = record
            .polymarket
            .as_mut()
            .ok_or(RegistryError::MissingPolymarketBinding(id))?;

        // Remove old next token_id from reverse index
        if let Some(ref old) = pm.next {
            self.by_polymarket_token_id.remove(&old.token_id);
        }

        self.by_polymarket_token_id
            .insert(binding.token_id.clone(), id);
        pm.next = Some(binding);

        Ok(())
    }

    /// Promote next window to current: `next → current`, old current is unbound.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::NoNextWindow`] if there is no next window to promote.
    pub fn promote_polymarket_next(&mut self, id: InstrumentId) -> Result<(), RegistryError> {
        let record = self
            .by_id
            .get_mut(&id)
            .ok_or(RegistryError::MissingInstrument(id))?;
        let pm = record
            .polymarket
            .as_mut()
            .ok_or(RegistryError::MissingPolymarketBinding(id))?;

        let next = pm.next.take().ok_or(RegistryError::NoNextWindow(id))?;

        // Remove old current from reverse index
        if let Some(ref old) = pm.current {
            self.by_polymarket_token_id.remove(&old.token_id);
        }

        // next is already in the reverse index (added by bind_polymarket_next)
        pm.current = Some(next);

        Ok(())
    }

    /// Unbind a Polymarket market entirely (both current and next).
    ///
    /// Called when a market resolves/expires. Removes token IDs from the
    /// reverse index but preserves the `InstrumentId` (stable identity).
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument doesn't exist.
    pub fn unbind_polymarket(&mut self, id: InstrumentId) -> Result<(), RegistryError> {
        let record = self
            .by_id
            .get_mut(&id)
            .ok_or(RegistryError::MissingInstrument(id))?;
        let pm = record
            .polymarket
            .as_mut()
            .ok_or(RegistryError::MissingPolymarketBinding(id))?;

        if let Some(ref current) = pm.current {
            self.by_polymarket_token_id.remove(&current.token_id);
        }
        if let Some(ref next) = pm.next {
            self.by_polymarket_token_id.remove(&next.token_id);
        }

        pm.current = None;
        pm.next = None;

        Ok(())
    }
}

impl<const D: u8> Default for InstrumentRegistry<D> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use mantis_fixed::FixedI64;
    use mantis_types::Timestamp;

    use super::*;

    fn poly_meta() -> InstrumentMeta<6> {
        // tick_size = 0.01, lot_size = 0.01
        InstrumentMeta::new(FixedI64::from_raw(10_000), FixedI64::from_raw(10_000)).unwrap()
    }

    fn window(token: &str, slug: &str, start: u64, end: u64) -> PolymarketWindowBinding {
        PolymarketWindowBinding {
            token_id: token.to_owned(),
            market_slug: slug.to_owned(),
            window_start: Timestamp::from_nanos(start),
            window_end: Timestamp::from_nanos(end),
            condition_id: Some("0xcondition".to_owned()),
        }
    }

    #[test]
    fn insert_and_lookup() {
        let mut reg = InstrumentRegistry::<6>::new();
        let key = InstrumentKey::prediction(Asset::Btc, Timeframe::M15, OutcomeSide::Up);
        let id = reg.insert(key.clone(), poly_meta(), None, None).unwrap();

        assert_eq!(id.to_raw(), 1);
        assert_eq!(reg.by_key(&key), Some(id));
        assert!(reg.meta(id).is_some());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn insert_prediction_pair() {
        let mut reg = InstrumentRegistry::<6>::new();
        let (up, down) = reg
            .insert_prediction_pair(Asset::Btc, Timeframe::M15, poly_meta(), Some("BTCUSDT"))
            .unwrap();

        assert_eq!(up.to_raw(), 1);
        assert_eq!(down.to_raw(), 2);
        assert_eq!(reg.by_binance_symbol("BTCUSDT"), Some(up));
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn duplicate_key_rejected() {
        let mut reg = InstrumentRegistry::<6>::new();
        let key = InstrumentKey::prediction(Asset::Btc, Timeframe::M15, OutcomeSide::Up);
        reg.insert(key.clone(), poly_meta(), None, None).unwrap();
        assert!(reg.insert(key, poly_meta(), None, None).is_err());
    }

    #[test]
    fn polymarket_bind_current() {
        let mut reg = InstrumentRegistry::<6>::new();
        let (up, _) = reg
            .insert_prediction_pair(Asset::Btc, Timeframe::M15, poly_meta(), None)
            .unwrap();

        reg.bind_polymarket_current(up, window("token_a", "btc-15m-1", 100, 200))
            .unwrap();

        assert_eq!(reg.by_polymarket_token_id("token_a"), Some(up));
    }

    #[test]
    fn polymarket_bind_next_and_promote() {
        let mut reg = InstrumentRegistry::<6>::new();
        let (up, _) = reg
            .insert_prediction_pair(Asset::Btc, Timeframe::M15, poly_meta(), None)
            .unwrap();

        // Bind current window
        reg.bind_polymarket_current(up, window("token_w1", "btc-15m-1", 100, 200))
            .unwrap();
        assert_eq!(reg.by_polymarket_token_id("token_w1"), Some(up));

        // Bind next window
        reg.bind_polymarket_next(up, window("token_w2", "btc-15m-2", 200, 300))
            .unwrap();
        assert_eq!(reg.by_polymarket_token_id("token_w2"), Some(up));

        // Both resolve to the same InstrumentId
        assert_eq!(reg.by_polymarket_token_id("token_w1"), Some(up));
        assert_eq!(reg.by_polymarket_token_id("token_w2"), Some(up));

        // Promote: next → current, old current gone
        reg.promote_polymarket_next(up).unwrap();
        assert_eq!(reg.by_polymarket_token_id("token_w2"), Some(up));
        assert_eq!(reg.by_polymarket_token_id("token_w1"), None);

        // InstrumentId is unchanged
        assert_eq!(up.to_raw(), 1);
    }

    #[test]
    fn polymarket_unbind() {
        let mut reg = InstrumentRegistry::<6>::new();
        let (up, _) = reg
            .insert_prediction_pair(Asset::Btc, Timeframe::M15, poly_meta(), None)
            .unwrap();

        reg.bind_polymarket_current(up, window("token_c", "slug", 100, 200))
            .unwrap();
        reg.bind_polymarket_next(up, window("token_n", "slug2", 200, 300))
            .unwrap();

        reg.unbind_polymarket(up).unwrap();

        assert_eq!(reg.by_polymarket_token_id("token_c"), None);
        assert_eq!(reg.by_polymarket_token_id("token_n"), None);
        // InstrumentId still exists, just unbound
        assert!(reg.get(up).is_some());
    }

    #[test]
    fn rebind_replaces_old_token() {
        let mut reg = InstrumentRegistry::<6>::new();
        let (up, _) = reg
            .insert_prediction_pair(Asset::Btc, Timeframe::M15, poly_meta(), None)
            .unwrap();

        reg.bind_polymarket_current(up, window("old_token", "slug1", 100, 200))
            .unwrap();
        assert_eq!(reg.by_polymarket_token_id("old_token"), Some(up));

        // Rebind with new token — old token should be removed
        reg.bind_polymarket_current(up, window("new_token", "slug2", 200, 300))
            .unwrap();
        assert_eq!(reg.by_polymarket_token_id("new_token"), Some(up));
        assert_eq!(reg.by_polymarket_token_id("old_token"), None);
    }

    #[test]
    fn promote_without_next_fails() {
        let mut reg = InstrumentRegistry::<6>::new();
        let (up, _) = reg
            .insert_prediction_pair(Asset::Btc, Timeframe::M15, poly_meta(), None)
            .unwrap();

        let result = reg.promote_polymarket_next(up);
        assert!(result.is_err());
    }

    #[test]
    fn active_token_ids() {
        let mut reg = InstrumentRegistry::<6>::new();
        let (up, down) = reg
            .insert_prediction_pair(Asset::Btc, Timeframe::M15, poly_meta(), None)
            .unwrap();

        assert!(reg.active_polymarket_token_ids().is_empty());

        reg.bind_polymarket_current(up, window("up_tok", "s1", 100, 200))
            .unwrap();
        reg.bind_polymarket_current(down, window("dn_tok", "s1", 100, 200))
            .unwrap();

        let mut active = reg.active_polymarket_token_ids();
        active.sort();
        assert_eq!(active, vec!["dn_tok", "up_tok"]);
    }

    #[test]
    fn instrument_key_symbol() {
        let key = InstrumentKey::prediction(Asset::Btc, Timeframe::M15, OutcomeSide::Up);
        assert_eq!(key.symbol(), "BTC-M15-Up");

        let ref_key = InstrumentKey::spot_reference(Asset::Btc, Asset::Usdt);
        assert_eq!(ref_key.symbol(), "BTC-USDT-Ref");
    }

    #[test]
    fn full_rotation_lifecycle() {
        let mut reg = InstrumentRegistry::<6>::new();
        let (up, down) = reg
            .insert_prediction_pair(Asset::Btc, Timeframe::M15, poly_meta(), Some("BTCUSDT"))
            .unwrap();

        // Window 1: bind current
        reg.bind_polymarket_current(up, window("w1_up", "btc-15m-w1", 0, 900))
            .unwrap();
        reg.bind_polymarket_current(down, window("w1_dn", "btc-15m-w1", 0, 900))
            .unwrap();

        // Pre-subscribe window 2
        reg.bind_polymarket_next(up, window("w2_up", "btc-15m-w2", 900, 1800))
            .unwrap();
        reg.bind_polymarket_next(down, window("w2_dn", "btc-15m-w2", 900, 1800))
            .unwrap();

        // All 4 tokens resolve
        assert_eq!(reg.by_polymarket_token_id("w1_up"), Some(up));
        assert_eq!(reg.by_polymarket_token_id("w2_up"), Some(up));
        assert_eq!(reg.by_polymarket_token_id("w1_dn"), Some(down));
        assert_eq!(reg.by_polymarket_token_id("w2_dn"), Some(down));

        // Window 1 expires → promote
        reg.promote_polymarket_next(up).unwrap();
        reg.promote_polymarket_next(down).unwrap();

        // Only window 2 tokens remain
        assert_eq!(reg.by_polymarket_token_id("w1_up"), None);
        assert_eq!(reg.by_polymarket_token_id("w2_up"), Some(up));
        assert_eq!(reg.by_polymarket_token_id("w1_dn"), None);
        assert_eq!(reg.by_polymarket_token_id("w2_dn"), Some(down));

        // InstrumentIds unchanged throughout
        assert_eq!(up.to_raw(), 1);
        assert_eq!(down.to_raw(), 2);
        assert_eq!(reg.by_binance_symbol("BTCUSDT"), Some(up));
    }
}
