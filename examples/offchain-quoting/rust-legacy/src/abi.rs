//! ABI for the legacy Pool contract.
//!
//! Differences vs the current Pool ABI:
//! - sqrt-price is published as `uint160 pX96` (Q96), not `uint80 pX48` (Q48)
//! - `state()` returns `(uint160 pX96, uint48 fee, uint48 latestUpdateBlock)`
//! - no separate `anchorPrice()` view: the operator-published `pX96` *is* the
//!   anchor and is also the post-update sqrt-price
//! - `StateUpdated.state` carries `(uint160 pX96, uint48 fee)`

use alloy::sol;

sol! {
    #[sol(rpc)]
    #[allow(missing_docs)]
    contract Pool {
        struct StateUpdateParameters {
            uint160 pX96;
            uint48 fee;
        }

        function X() external view returns (address);
        function Y() external view returns (address);
        function state() external view returns (uint160 pX96, uint48 fee, uint48 latestUpdateBlock);
        function concentrationK() external view returns (uint32);
        function blockDelay() external view returns (uint48);
        function paused() external view returns (bool);
        function getXReserve() external view returns (uint112);
        function getYReserve() external view returns (uint112);
        function isFresh() external view returns (bool);

        event StateUpdated(StateUpdateParameters state);
        event Sync(uint128 reserveX, uint128 reserveY);
        event SwapExecuted(address recipient, bool xToY, uint256 dx, uint256 dy, uint256 fee);
        event ConcentrationKSet(uint32 concentrationK);
        event BlockDelaySet(uint48 blockDelay);
        event Paused(address account);
        event Unpaused(address account);
    }
}
