use alloy::sol;

sol! {
    #[sol(rpc)]
    #[allow(missing_docs)]
    contract Pool {
        function X() external view returns (address);
        function Y() external view returns (address);
        function state() external view returns (
            uint80 anchorPrice,
            uint80 pX48,
            uint24 feeAskX24,
            uint24 feeBidX24,
            uint48 latestUpdateBlock
        );
        function anchorPrice() external view returns (uint80);
        function concentrationKQ12() external view returns (uint32);
        function blockDelay() external view returns (uint48);
        function paused() external view returns (bool);
        function getXReserve() external view returns (uint112);
        function getYReserve() external view returns (uint112);

        event StateUpdated(uint80 anchorPrice, uint24 feeAskX24, uint24 feeBidX24);
        event Sync(uint128 reserveX, uint128 reserveY);
        event SwapExecuted(address recipient, bool xToY, uint256 dx, uint256 dy, uint256 fee);
        event ConcentrationKQ12Set(uint32 concentrationKQ12);
        event BlockDelaySet(uint48 blockDelay);
        event Paused(address account);
        event Unpaused(address account);
    }
}
