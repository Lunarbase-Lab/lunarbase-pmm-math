// SPDX-License-Identifier: MIT
pragma solidity 0.8.31;

import {PoolVectorOracle} from "./utils/PoolVectorOracle.sol";

/// @title Canonical deterministic JSONL v4 oracle for Pool quotes and punishment transitions.
contract PoolDeterministicParityOracle is PoolVectorOracle {
    string internal constant VECTORS_DIR = "generated";
    string internal constant VECTORS_PATH = "generated/deterministic_vectors.jsonl";

    function setUp() public {
        vm.createDir(VECTORS_DIR, true);
        _setUpVectorOracle();
    }

    function test_writeDeterministicVectors() public {
        _writeNamed(
            "base_x_to_y_no_punishment",
            _vector(true, Q96, 101, 50_331, uint112(1000 ether), uint112(1000 ether), 0, 1, 1 ether)
        );
        _writeNamed(
            "base_y_to_x_asymmetric_fees",
            _vector(false, Q96, 167_772, 50_331, uint112(1000 ether), uint112(1000 ether), 16_778, 1, 1 ether)
        );

        // sqrt prices 2 and 1/2 correspond to raw prices 4 and 1/4.
        _writeNamed(
            "non_unit_price_x_to_y",
            _vector(true, Q96 * 2, 3000, 5000, uint112(100 ether), uint112(1000 ether), 16_778, 1, 10 ether)
        );
        _writeNamed(
            "non_unit_price_y_to_x",
            _vector(false, Q96 / 2, 5000, 3000, uint112(1000 ether), uint112(100 ether), 16_778, 1, 10 ether)
        );

        _writeNamed(
            "fee_multiplier_x_to_y",
            _vector(true, Q96, 10_000, 100_000, uint112(1000 ether), uint112(1000 ether), 16_778, 3, 25 ether)
        );
        _writeNamed(
            "fee_multiplier_y_to_x",
            _vector(false, Q96, 100_000, 10_000, uint112(1000 ether), uint112(1000 ether), 16_778, 7, 25 ether)
        );

        // The max sentinel expands to conceptual Q24 before ceil division.
        _writeNamed(
            "max_punishment_sentinel_one_third",
            _vector(true, Q96, 0, 0, uint112(450 ether), uint112(450 ether), FULL_FEE_X24, 1, 300 ether)
        );

        // Immediate punishment reaches the full-fee sentinel and blocks the
        // swap without persisting the counterfactual effective fee.
        _writeNamed(
            "blocked_saturating_bid_headroom",
            _vector(true, Q96, 0, FULL_FEE_X24 - 5, 0, uint112(1000 ether), FULL_FEE_X24, 1, 1000 ether)
        );
        _writeNamed(
            "blocked_saturating_ask_headroom",
            _vector(false, Q96, FULL_FEE_X24 - 7, 0, uint112(1000 ether), 0, FULL_FEE_X24, 1, 1000 ether)
        );
        _writeNamed(
            "max_and_fee_minus_one",
            _vector(true, Q96, 0, FULL_FEE_X24 - 1, 0, uint112(1000 ether), FULL_FEE_X24 - 1, 1, 1000 ether)
        );

        _writeNamed("one_sided_x_to_y", _vector(true, Q96, 0, 0, 0, uint112(1000 ether), 16_778, 1, 10 ether));
        _writeNamed("one_sided_y_to_x", _vector(false, Q96, 0, 0, uint112(1000 ether), 0, 16_778, 1, 10 ether));

        // Unit amounts exercise both valuation floors plus ceil punishment rounding.
        _writeNamed("rounding_x_to_y", _vector(true, Q96, 0, 1, 1, 2, 1, 1, 1));
        _writeNamed("rounding_y_to_x", _vector(false, Q96, 1, 0, 2, 1, 1, 1, 1));

        // Quote-level rejection rows are retained with their observed no-op post state.
        _writeNamed("rejected_output_reserve", _vector(true, Q96, 0, 0, 100, 1, 16_778, 1, 2));
        _writeNamed(
            "rejected_full_fee_sentinel",
            _vector(false, Q96, FULL_FEE_X24, 0, uint112(100 ether), uint112(100 ether), FULL_FEE_X24, 1, 10 ether)
        );
        _writeNamed(
            "rejected_multiplier_overflow",
            _vector(true, Q96, 0, 1, uint112(100 ether), uint112(100 ether), 16_778, type(uint256).max, 10 ether)
        );
        _writeNamed("rejected_zero_anchor", _vector(false, 0, 0, 0, 100, 100, FULL_FEE_X24, 1, 1));

        // Quote succeeds, but input-side reserve sync cannot fit uint112. The full transaction rolls back.
        _writeNamed(
            "rollback_reserve_transition_overflow",
            _vector(true, Q96, 0, 0, type(uint112).max, uint112(100 ether), FULL_FEE_X24, 1, 1 ether)
        );

        // Both are valid ABI inputs whose first quote mulDiv result cannot fit uint256.
        _writeNamed(
            "math_muldiv_revert_x_to_y_max_input_max_anchor",
            _vector(
                true, type(uint160).max, 0, 0, type(uint112).max, type(uint112).max, FULL_FEE_X24, 1, type(uint256).max
            )
        );
        _writeNamed(
            "math_muldiv_revert_y_to_x_max_input_unit_anchor",
            _vector(false, 1, 0, 0, type(uint112).max, type(uint112).max, FULL_FEE_X24, 1, type(uint256).max)
        );

        // A half-inventory swap applies an immediate 50% punishment, then a
        // real Pool.upd replaces both effective fees without changing reserves
        // or max punishment.
        _writeNamedUpdate(
            "extreme_punishment_then_operator_update",
            _vector(true, Q96, 123, 0, 0, type(uint112).max, FULL_FEE_X24, 1, type(uint112).max / 2),
            Q96 * 2,
            111,
            222
        );

        _writeNamed(
            "punishment_equal_to_headroom_blocks",
            _vector(true, Q96, 0, FULL_FEE_X24 - 5, 0, uint112(1000 ether), 5, 1, 1000 ether)
        );
        _writeNamed(
            "punishment_below_headroom_applies",
            _vector(true, Q96, 0, FULL_FEE_X24 - 5, 0, uint112(1000 ether), 4, 1, 1000 ether)
        );
        _writeNamed(
            "full_punishment_from_zero_blocks",
            _vector(true, Q96, 0, 0, 0, uint112(1000 ether), FULL_FEE_X24, 1, 1000 ether)
        );
        _writeNamed(
            "zero_max_y_to_x_uses_stored_ask",
            _vector(false, Q96, 12_345, 678, uint112(1000 ether), uint112(1000 ether), 0, 1, 1 ether)
        );
        _writeNamed(
            "rollback_reserve_transition_overflow_y_to_x",
            _vector(false, Q96, 0, 0, uint112(100 ether), type(uint112).max, FULL_FEE_X24, 1, 1 ether)
        );

        _writeSplitSequence();
        _writeFullMaxSplitSequence();
    }

    function _writeNamed(string memory name, VectorInput memory input) private returns (VectorOutput memory output) {
        output = _writeVector(VECTORS_PATH, "name", name, input);
    }

    function _writeSplitSequence() private {
        uint256 splitCount = 10;
        uint256 amountPerSwap = 10_000 ether;
        uint256 totalAmountIn = amountPerSwap * splitCount;
        uint24 liveFeeX24 = 3_644;
        uint24 maxPunishmentX24 = 1_677_722;
        uint112 sideReserve = uint112(500_000 ether);

        VectorInput memory base =
            _vector(true, Q96, liveFeeX24, liveFeeX24, sideReserve, sideReserve, maxPunishmentX24, 1, totalAmountIn);
        VectorOutput memory single = _writeNamed("split_single_immediate", base);
        require(single.outcome == VectorOutcome.Applied, "single split reference must apply");

        base.amountIn = amountPerSwap;
        uint256 splitTotalAmountOut;
        uint256 splitTotalFees;
        uint24 finalSplitBidX24;
        for (uint256 i; i < splitCount; ++i) {
            VectorOutput memory step = _writeNamed(string.concat("split_step_", vm.toString(i)), base);
            require(step.outcome == VectorOutcome.Applied, "split step must apply");
            splitTotalAmountOut += step.amountOut;
            splitTotalFees += step.feeAmount;
            finalSplitBidX24 = step.feeBidX24After;

            base.feeAskX24 = step.feeAskX24After;
            base.feeBidX24 = step.feeBidX24After;
            base.reserveX = step.reserveXAfter;
            base.reserveY = step.reserveYAfter;
        }

        assertLt(splitTotalFees, single.feeAmount, "split sequence did not reduce integrated fee");
        assertGt(splitTotalAmountOut, single.amountOut, "split sequence did not increase output");
        assertGe(finalSplitBidX24, single.feeBidX24After, "split sequence ended below single fee state");
        assertLe(
            uint256(finalSplitBidX24) - uint256(single.feeBidX24After),
            splitCount - 1,
            "split state exceeded per-call ceil tolerance"
        );
    }

    function _writeFullMaxSplitSequence() private {
        uint256 splitCount = 10;
        uint256 amountPerSwap = 100_000 ether;
        uint112 sideReserve = uint112(1_000_000 ether);

        VectorInput memory base =
            _vector(true, Q96, 0, 0, sideReserve, sideReserve, FULL_FEE_X24, 1, amountPerSwap * splitCount);
        VectorOutput memory single = _writeNamed("full_max_split_single", base);
        require(single.outcome == VectorOutcome.Applied, "full-max single must apply");
        assertEq(single.amountOut, 500_000 ether, "unexpected full-max single output");
        assertEq(single.feeBidX24After, 8_388_608, "unexpected full-max single fee state");

        base.amountIn = amountPerSwap;
        uint256 splitTotalAmountOut;
        uint24 finalSplitBidX24;
        for (uint256 i; i < splitCount; ++i) {
            VectorOutput memory step = _writeNamed(string.concat("full_max_split_step_", vm.toString(i)), base);
            require(step.outcome == VectorOutcome.Applied, "full-max split step must apply");
            splitTotalAmountOut += step.amountOut;
            finalSplitBidX24 = step.feeBidX24After;

            base.feeAskX24 = step.feeAskX24After;
            base.feeBidX24 = step.feeBidX24After;
            base.reserveX = step.reserveXAfter;
            base.reserveY = step.reserveYAfter;
        }

        assertEq(splitTotalAmountOut, 724_999_934_434_890_747_070_315, "unexpected full-max split output");
        assertEq(finalSplitBidX24, 8_388_610, "unexpected full-max split fee state");
        assertGt(splitTotalAmountOut, single.amountOut, "full-max split did not increase output");
    }

    function _writeNamedUpdate(
        string memory name,
        VectorInput memory input,
        uint160 updateAnchorPrice,
        uint24 updateFeeAskX24,
        uint24 updateFeeBidX24
    ) private {
        _writeUpdateVector(VECTORS_PATH, "name", name, input, updateAnchorPrice, updateFeeAskX24, updateFeeBidX24);
    }

    function _vector(
        bool xToY,
        uint160 anchorPrice,
        uint24 feeAskX24,
        uint24 feeBidX24,
        uint112 reserveX,
        uint112 reserveY,
        uint24 maxPunishmentX24,
        uint256 feeMultiplier,
        uint256 amountIn
    ) private pure returns (VectorInput memory input) {
        input = VectorInput({
            xToY: xToY,
            anchorPrice: anchorPrice,
            feeAskX24: feeAskX24,
            feeBidX24: feeBidX24,
            reserveX: reserveX,
            reserveY: reserveY,
            maxPunishmentX24: maxPunishmentX24,
            feeMultiplier: feeMultiplier,
            amountIn: amountIn
        });
    }
}
