// SPDX-License-Identifier: MIT
pragma solidity 0.8.31;

import {PoolVectorOracle, ParityPool, PoolVectorToken} from "./utils/PoolVectorOracle.sol";
import {ExactInputParams} from "@/types/PoolTypes.sol";

contract ActiveTreasuryParity is PoolVectorOracle {
    function setUp() public {
        _setUpVectorOracle();
    }

    function testFuzz_partnerFeesAreTheOnlyFeeReserveDebit(
        uint32 shareSeed,
        uint64 amountSeed,
        bool reverse,
        bool hasOperator
    ) public {
        uint32 share = uint32(uint256(shareSeed) % 1_000_001);
        uint256 amount = 100 + uint256(amountSeed) % 100_000;
        ParityPool pool = _pool(1_000_000, 1_677_721, 167_772);
        pool.setPartnerFee(address(this), share);
        if (hasOperator) pool.setPartnerOperator(address(this), address(0xA11CE));
        (uint256 expectedOutput,, uint256 fee) = reverse ? pool.quoteYToX(amount) : pool.quoteXToY(amount);
        _swap(pool, amount, reverse);
        uint256 partner = hasOperator ? fee * share / 1_000_000 : 0;
        uint256 reserve = reverse ? pool.getXReserve() : pool.getYReserve();
        assertEq(reserve, 1_000_000 - expectedOutput - partner);
        (uint112 treasuryX, uint112 treasuryY) = pool.treasuryFees();
        assertEq(reverse ? treasuryX : treasuryY, fee - fee * share / 1_000_000);
    }

    function _pool(uint112 reserve, uint24 fee, uint24 maximum) private returns (ParityPool pool) {
        vm.roll(1000);
        address[] memory operators = new address[](1);
        operators[0] = VECTOR_OPERATOR;
        pool = new ParityPool(address(tokenX), address(tokenY), address(this), operators);
        tokenX.mint(address(pool), reserve);
        tokenY.mint(address(pool), reserve);
        pool.syncState();
        vm.prank(VECTOR_OPERATOR);
        pool.upd(Q96, fee, fee, block.number);
        pool.setMaxPunishmentX24(maximum);
        pool.unpause();
    }

    function _swap(ParityPool pool, uint256 amount, bool reverse) private returns (uint256) {
        PoolVectorToken input = reverse ? tokenY : tokenX;
        input.mint(address(this), amount);
        input.approve(address(pool), amount);
        return pool.swapExactIn(
            ExactInputParams({
                tokenIn: address(input),
                tokenOut: reverse ? address(tokenX) : address(tokenY),
                recipient: VECTOR_RECIPIENT,
                amountIn: amount,
                amountOutMinimum: 0,
                deadline: block.timestamp
            })
        );
    }
}
