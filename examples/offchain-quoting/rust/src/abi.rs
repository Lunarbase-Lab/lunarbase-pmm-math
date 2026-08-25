use alloy::sol;

sol! {
    #[sol(rpc)]
    #[allow(missing_docs)]
    contract Pool {
        function X() external view returns (address);
        function Y() external view returns (address);
        function state() external view returns (
            uint160 anchorPrice,
            uint24 feeAskX24,
            uint24 feeBidX24,
            uint48 latestUpdateBlock
        );
        function anchorPrice() external view returns (uint160);
        function maxPunishmentX24() external view returns (uint24);
        function blockDelay() external view returns (uint48);
        function paused() external view returns (bool);
        function getXReserve() external view returns (uint112);
        function getYReserve() external view returns (uint112);
        function blacklistFeeMultiplier() external view returns (uint256);
        function isWhitelisted(address account) external view returns (bool);
    }
}
