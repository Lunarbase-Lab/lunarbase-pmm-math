// SPDX-License-Identifier: MIT
pragma solidity 0.8.31;

import {SafeCast} from "@openzeppelin/contracts/utils/math/SafeCast.sol";
import {PoolVectorOracle} from "./utils/PoolVectorOracle.sol";

/// @title Seeded JSONL v4 oracle for quote and punishment differential testing.
/// @dev Run single-threaded because every fuzz run appends two rows to one file.
contract PoolFuzzParityOracle is PoolVectorOracle {
    using SafeCast for uint256;

    string internal constant VECTORS_DIR = "generated";
    string internal constant VECTORS_PATH = "generated/fuzz_vectors.jsonl";

    function setUp() public {
        vm.createDir(VECTORS_DIR, true);
        _setUpVectorOracle();
    }

    /// @dev No assumptions or early returns: each run emits one row per direction.
    function testFuzz_writeQuoteAndPunishmentVectors(uint256 seed) public {
        _writeSeed(seed, _seededVector(seed, true));
        _writeSeed(seed, _seededVector(seed, false));
    }

    function _writeSeed(uint256 seed, VectorInput memory input) private {
        _writeVector(VECTORS_PATH, "seed", vm.toString(seed), input);
    }

    function _seededVector(uint256 seed, bool xToY) private pure returns (VectorInput memory input) {
        uint256 r0 = uint256(keccak256(abi.encode(seed, xToY, uint256(0))));
        uint256 r1 = uint256(keccak256(abi.encode(seed, xToY, uint256(1))));
        uint256 r2 = uint256(keccak256(abi.encode(seed, xToY, uint256(2))));
        uint256 r3 = uint256(keccak256(abi.encode(seed, xToY, uint256(3))));

        uint160 anchorPrice = (uint256(Q96) / 2 + (r0 % ((uint256(Q96) * 3) / 2 + 1))).toUint160();
        uint112 reserveX = (1 ether + (r1 % (1_000_000 ether))).toUint112();
        uint112 reserveY = (1 ether + (r2 % (1_000_000 ether))).toUint112();
        uint24 feeAskX24 = (r1 % (Q24 / 20)).toUint24();
        uint24 feeBidX24 = (r2 % (Q24 / 20)).toUint24();
        uint24 maxPunishmentX24 = (r3 & type(uint24).max).toUint24();
        uint256 feeMultiplier = 1 + (r3 % 5);
        uint256 outputReserve = xToY ? reserveY : reserveX;

        input = _vector(
            xToY,
            anchorPrice,
            feeAskX24,
            feeBidX24,
            reserveX,
            reserveY,
            maxPunishmentX24,
            feeMultiplier,
            1 + (outputReserve / 16)
        );

        uint256 mode = r0 % 12;
        if (mode == 0) {
            input.maxPunishmentX24 = 0;
            _setDirectionalFee(input, 0);
            input.feeMultiplier = 1;
        } else if (mode == 1) {
            _setDirectionalFee(input, 0);
            input.feeMultiplier = 1;
            input.amountIn = outputReserve * 8 + 1;
        } else if (mode == 2) {
            _setDirectionalFee(input, 10_000);
            input.feeMultiplier = type(uint256).max;
        } else if (mode == 3) {
            _setDirectionalFee(input, FULL_FEE_X24);
            input.feeMultiplier = 1;
        } else if (mode == 4) {
            input.anchorPrice = Q96;
            input.maxPunishmentX24 = FULL_FEE_X24;
            _setDirectionalFee(input, FULL_FEE_X24 - (1 + (r3 % 64)).toUint24());
            _setOneSided(input, uint112(1000 ether));
            input.amountIn = 1000 ether;
            input.feeMultiplier = 1;
        } else if (mode == 5) {
            input.anchorPrice = Q96;
            _setDirectionalFee(input, (r3 % 100_000).toUint24());
            _setOneSided(input, uint112(1000 ether));
            input.amountIn = 1 + (r3 % (100 ether));
            input.feeMultiplier = 1 + (r2 % 3);
        } else if (mode == 6) {
            input.anchorPrice = Q96;
            input.reserveX = 2;
            input.reserveY = 2;
            input.maxPunishmentX24 = (1 + (r3 % 16)).toUint24();
            _setDirectionalFee(input, (r2 % 2).toUint24());
            input.amountIn = 1;
            input.feeMultiplier = 1;
        } else if (mode == 7) {
            input.anchorPrice = Q96;
            input.maxPunishmentX24 = FULL_FEE_X24 - 1;
            _setDirectionalFee(input, FULL_FEE_X24 - 1);
            _setOneSided(input, uint112(1000 ether));
            input.amountIn = 1000 ether;
            input.feeMultiplier = 1;
        } else if (mode == 8) {
            input.anchorPrice = Q96;
            input.maxPunishmentX24 = FULL_FEE_X24;
            _setDirectionalFee(input, 0);
            _setInputReserve(input, type(uint112).max);
            _setOutputReserve(input, uint112(100 ether));
            input.amountIn = 1 ether;
            input.feeMultiplier = 1;
        } else if (mode == 9) {
            input.anchorPrice = 0;
            _setDirectionalFee(input, 0);
            input.amountIn = 1;
            input.feeMultiplier = 1;
        } else if (mode == 10) {
            input.anchorPrice = xToY ? Q96 * 2 : Q96 / 2;
            _setDirectionalFee(input, (r2 % 100_000).toUint24());
            input.amountIn = 1 + (outputReserve / 16);
            input.feeMultiplier = 1 + (r1 % 3);
        } else {
            input.maxPunishmentX24 = FULL_FEE_X24;
            input.amountIn = 0;
            input.feeMultiplier = 1;
        }
    }

    function _setDirectionalFee(VectorInput memory input, uint24 feeX24) private pure {
        if (input.xToY) input.feeBidX24 = feeX24;
        else input.feeAskX24 = feeX24;
    }

    function _setOneSided(VectorInput memory input, uint112 outputReserve) private pure {
        _setInputReserve(input, 0);
        _setOutputReserve(input, outputReserve);
    }

    function _setInputReserve(VectorInput memory input, uint112 reserve) private pure {
        if (input.xToY) input.reserveX = reserve;
        else input.reserveY = reserve;
    }

    function _setOutputReserve(VectorInput memory input, uint112 reserve) private pure {
        if (input.xToY) input.reserveY = reserve;
        else input.reserveX = reserve;
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
