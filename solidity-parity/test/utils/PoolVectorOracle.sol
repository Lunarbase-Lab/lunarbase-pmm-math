// SPDX-License-Identifier: MIT
pragma solidity 0.8.31;

import {Test} from "forge-std/Test.sol";
import {Pool} from "@/Pool.sol";
import {IPoolErrors} from "@/interfaces/errors/IPoolErrors.sol";
import {PoolBridgeLib} from "@/libraries/PoolBridgeLib.sol";
import {SwapLib} from "@/libraries/SwapLib.sol";
import {PoolCoreStorage} from "@/types/PoolCoreStorage.sol";
import {ExactInputParams} from "@/types/PoolTypes.sol";

/// @dev Exact-transfer ERC-20 used by the portable Solidity oracle.
contract PoolVectorToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

/// @dev Production Pool with only the reserve-sync test hook required for setup.
contract ParityPool is Pool {
    using PoolBridgeLib for PoolCoreStorage.Layout;

    constructor(address x, address y, address initialOwner, address[] memory operators)
        Pool(x, y, initialOwner, operators)
    {}

    function syncState() external {
        PoolCoreStorage.layout().sync(X(), Y());
    }
}

/// @dev Exposes only the pure punishment calculation required by off-chain ports.
contract PoolPunishmentVectorHarness {
    function punishmentX24(
        uint160 anchorPrice,
        uint256 amountIn,
        uint112 reserveX,
        uint112 reserveY,
        uint24 maxPunishmentX24,
        bool xToY
    ) external pure returns (uint24) {
        return SwapLib.punishmentX24(anchorPrice, amountIn, reserveX, reserveY, maxPunishmentX24, xToY);
    }

    function saturatingFeeX24(uint24 storedFeeX24, uint24 desiredPunishmentX24) external pure returns (uint24) {
        return SwapLib.saturatingFeeX24(storedFeeX24, desiredPunishmentX24);
    }
}

/// @dev Shared real-Pool execution and JSONL v4 serialization for parity vectors.
abstract contract PoolVectorOracle is Test {
    uint160 internal constant Q96 = uint160(1 << 96);
    uint256 internal constant Q24 = 1 << 24;
    uint24 internal constant FULL_FEE_X24 = type(uint24).max;
    bytes4 internal constant SAFE_CAST_OVERFLOW_SELECTOR =
        bytes4(keccak256("SafeCastOverflowedUintDowncast(uint8,uint256)"));
    bytes4 internal constant PANIC_SELECTOR = bytes4(keccak256("Panic(uint256)"));
    uint256 internal constant PANIC_ARITHMETIC_UNDER_OVERFLOW = 0x11;

    enum VectorOutcome {
        Applied,
        SwapImpossible,
        ReserveTransitionOverflow,
        MathMulDivRevert
    }

    struct VectorInput {
        bool xToY;
        uint160 anchorPrice;
        uint24 feeAskX24;
        uint24 feeBidX24;
        uint112 reserveX;
        uint112 reserveY;
        uint24 maxPunishmentX24;
        uint256 feeMultiplier;
        uint256 amountIn;
    }

    struct VectorOutput {
        uint256 amountOut;
        uint160 pNext;
        uint256 feeAmount;
        VectorOutcome outcome;
        uint24 desiredPunishmentX24;
        uint24 effectiveFeeX24;
        uint24 appliedPunishmentX24;
        uint24 feeAskX24After;
        uint24 feeBidX24After;
        uint112 reserveXAfter;
        uint112 reserveYAfter;
    }

    struct UpdateOutput {
        bool enabled;
        uint160 anchorPrice;
        uint24 feeAskX24;
        uint24 feeBidX24;
        uint160 anchorPriceAfter;
        uint24 feeAskX24After;
        uint24 feeBidX24After;
        uint112 reserveXAfter;
        uint112 reserveYAfter;
        uint24 maxPunishmentX24After;
    }

    PoolVectorToken internal tokenX;
    PoolVectorToken internal tokenY;
    PoolPunishmentVectorHarness internal punishmentHarness;

    address internal constant VECTOR_OPERATOR = address(0xBEEF);
    address internal constant VECTOR_RECIPIENT = address(0xCAFE);

    function _setUpVectorOracle() internal {
        tokenX = new PoolVectorToken();
        tokenY = new PoolVectorToken();
        if (address(tokenX) > address(tokenY)) (tokenX, tokenY) = (tokenY, tokenX);
        punishmentHarness = new PoolPunishmentVectorHarness();
    }

    function _writeVector(
        string memory vectorsPath,
        string memory identityKey,
        string memory identityValue,
        VectorInput memory input
    ) internal returns (VectorOutput memory output) {
        UpdateOutput memory update;
        (output, update) = _executeVector(input, false, 0, 0, 0);
        vm.writeLine(vectorsPath, _serializeVector(identityKey, identityValue, input, output, update));
    }

    function _writeUpdateVector(
        string memory vectorsPath,
        string memory identityKey,
        string memory identityValue,
        VectorInput memory input,
        uint160 updateAnchorPrice,
        uint24 updateFeeAskX24,
        uint24 updateFeeBidX24
    ) internal {
        (VectorOutput memory output, UpdateOutput memory update) =
            _executeVector(input, true, updateAnchorPrice, updateFeeAskX24, updateFeeBidX24);
        vm.writeLine(vectorsPath, _serializeVector(identityKey, identityValue, input, output, update));
    }

    function _executeVector(
        VectorInput memory input,
        bool applyUpdate,
        uint160 updateAnchorPrice,
        uint24 updateFeeAskX24,
        uint24 updateFeeBidX24
    ) private returns (VectorOutput memory output, UpdateOutput memory update) {
        require(input.feeMultiplier > 0, "fee multiplier must be nonzero");

        address[] memory operators = new address[](1);
        operators[0] = VECTOR_OPERATOR;
        ParityPool pool = new ParityPool(address(tokenX), address(tokenY), address(this), operators);

        tokenX.mint(address(pool), input.reserveX);
        tokenY.mint(address(pool), input.reserveY);
        pool.syncState();

        vm.prank(VECTOR_OPERATOR);
        pool.upd(input.anchorPrice, input.feeAskX24, input.feeBidX24);
        pool.setMaxPunishmentX24(input.maxPunishmentX24);
        pool.setBlacklistFeeMultiplier(input.feeMultiplier);
        pool.unpause();

        bytes memory quoteRevert;
        if (input.xToY) {
            try pool.quoteXToY(input.amountIn) returns (uint256 amountOut, uint160 pNext, uint256 feeAmount) {
                (output.amountOut, output.pNext, output.feeAmount) = (amountOut, pNext, feeAmount);
            } catch (bytes memory reason) {
                quoteRevert = reason;
            }
        } else {
            try pool.quoteYToX(input.amountIn) returns (uint256 amountOut, uint160 pNext, uint256 feeAmount) {
                (output.amountOut, output.pNext, output.feeAmount) = (amountOut, pNext, feeAmount);
            } catch (bytes memory reason) {
                quoteRevert = reason;
            }
        }

        if (quoteRevert.length != 0) {
            _assertMathMulDivRevert(quoteRevert);
            output.outcome = VectorOutcome.MathMulDivRevert;
            _snapshot(pool, output);
            _assertTransition(input, output);
            require(!applyUpdate, "cannot update after reverted math");
            return (output, update);
        }

        // The production quote computes this increment before pricing and
        // saturating the current directional fee. Record it even when the
        // resulting zero-output swap is rejected and cannot persist state.
        output.desiredPunishmentX24 = punishmentHarness.punishmentX24(
            input.anchorPrice, input.amountIn, input.reserveX, input.reserveY, input.maxPunishmentX24, input.xToY
        );
        uint24 storedFeeX24 = input.xToY ? input.feeBidX24 : input.feeAskX24;
        output.effectiveFeeX24 = punishmentHarness.saturatingFeeX24(storedFeeX24, output.desiredPunishmentX24);

        PoolVectorToken inputToken = input.xToY ? tokenX : tokenY;
        ExactInputParams memory params = ExactInputParams({
            tokenIn: address(inputToken),
            tokenOut: input.xToY ? address(tokenY) : address(tokenX),
            recipient: VECTOR_RECIPIENT,
            amountIn: input.amountIn,
            amountOutMinimum: 0,
            deadline: block.timestamp
        });

        if (output.amountOut == 0) {
            try pool.swapExactIn(params) returns (uint256) {
                revert("expected SwapImpossible");
            } catch (bytes memory reason) {
                _assertExactSelector(reason, IPoolErrors.SwapImpossible.selector, 4);
                output.outcome = VectorOutcome.SwapImpossible;
            }
        } else {
            inputToken.mint(address(this), input.amountIn);
            inputToken.approve(address(pool), input.amountIn);
            try pool.swapExactIn(params) returns (uint256 actualAmountOut) {
                assertEq(actualAmountOut, output.amountOut, "swap diverged from triggering quote");
                output.outcome = VectorOutcome.Applied;
            } catch (bytes memory reason) {
                bytes4 selector = _selector(reason);
                if (selector == SAFE_CAST_OVERFLOW_SELECTOR) {
                    _assertReserveTransitionRevert(reason);
                    output.outcome = VectorOutcome.ReserveTransitionOverflow;
                } else if (_isMathMulDivRevert(reason)) {
                    _assertPunishmentMathRevert(input);
                    output.outcome = VectorOutcome.MathMulDivRevert;
                } else {
                    _revertWith(reason);
                }
            }
        }

        _snapshot(pool, output);

        uint24 directionalFeeBefore = input.xToY ? input.feeBidX24 : input.feeAskX24;
        uint24 directionalFeeAfter = input.xToY ? output.feeBidX24After : output.feeAskX24After;
        output.appliedPunishmentX24 = directionalFeeAfter - directionalFeeBefore;

        if (output.outcome != VectorOutcome.MathMulDivRevert) {
            assertEq(output.pNext, input.anchorPrice, "linear quote moved anchor price");
        }
        _assertTransition(input, output);

        if (applyUpdate) {
            require(output.outcome == VectorOutcome.Applied, "update vector swap must apply");
            require(output.appliedPunishmentX24 > 0, "update vector must first apply punishment");
            update = _applyAndRecordUpdate(
                pool, updateAnchorPrice, updateFeeAskX24, updateFeeBidX24, input.maxPunishmentX24
            );
        }
    }

    function _assertTransition(VectorInput memory input, VectorOutput memory output) private pure {
        if (output.outcome != VectorOutcome.MathMulDivRevert) {
            uint24 storedFeeX24 = input.xToY ? input.feeBidX24 : input.feeAskX24;
            uint256 widened = uint256(storedFeeX24) + uint256(output.desiredPunishmentX24);
            uint24 expectedEffectiveFeeX24 = widened >= FULL_FEE_X24 ? FULL_FEE_X24 : uint24(widened);
            assertEq(output.effectiveFeeX24, expectedEffectiveFeeX24, "unexpected effective fee");
        }

        if (output.outcome != VectorOutcome.Applied) {
            assertEq(output.appliedPunishmentX24, 0, "rejected swap applied punishment");
            assertEq(output.feeAskX24After, input.feeAskX24, "rejected swap changed ask fee");
            assertEq(output.feeBidX24After, input.feeBidX24, "rejected swap changed bid fee");
            assertEq(output.reserveXAfter, input.reserveX, "rejected swap changed X reserve");
            assertEq(output.reserveYAfter, input.reserveY, "rejected swap changed Y reserve");
            return;
        }

        uint24 directionalFeeBefore = input.xToY ? input.feeBidX24 : input.feeAskX24;
        uint256 available = uint256(FULL_FEE_X24) - uint256(directionalFeeBefore);
        uint24 expectedApplied =
            uint24(uint256(output.desiredPunishmentX24) < available ? uint256(output.desiredPunishmentX24) : available);
        assertEq(output.appliedPunishmentX24, expectedApplied, "unexpected punishment increment");
        uint24 directionalFeeAfter = input.xToY ? output.feeBidX24After : output.feeAskX24After;
        assertEq(directionalFeeAfter, output.effectiveFeeX24, "successful swap did not persist quote fee");

        uint256 grossOutput = output.amountOut + output.feeAmount;
        if (input.xToY) {
            assertEq(output.feeAskX24After, input.feeAskX24, "X to Y changed ask fee");
            assertEq(
                uint256(output.reserveXAfter),
                uint256(input.reserveX) + input.amountIn,
                "unexpected X reserve after X to Y"
            );
            assertEq(
                uint256(output.reserveYAfter),
                uint256(input.reserveY) - grossOutput,
                "unexpected Y reserve after X to Y"
            );
        } else {
            assertEq(output.feeBidX24After, input.feeBidX24, "Y to X changed bid fee");
            assertEq(
                uint256(output.reserveXAfter),
                uint256(input.reserveX) - grossOutput,
                "unexpected X reserve after Y to X"
            );
            assertEq(
                uint256(output.reserveYAfter),
                uint256(input.reserveY) + input.amountIn,
                "unexpected Y reserve after Y to X"
            );
        }
    }

    function _snapshot(ParityPool pool, VectorOutput memory output) private view {
        (, output.feeAskX24After, output.feeBidX24After,) = pool.state();
        output.reserveXAfter = pool.getXReserve();
        output.reserveYAfter = pool.getYReserve();
    }

    function _applyAndRecordUpdate(
        ParityPool pool,
        uint160 updateAnchorPrice,
        uint24 updateFeeAskX24,
        uint24 updateFeeBidX24,
        uint24 expectedMaxPunishmentX24
    ) private returns (UpdateOutput memory update) {
        update.enabled = true;
        update.anchorPrice = updateAnchorPrice;
        update.feeAskX24 = updateFeeAskX24;
        update.feeBidX24 = updateFeeBidX24;

        uint112 reserveXBefore = pool.getXReserve();
        uint112 reserveYBefore = pool.getYReserve();
        vm.prank(VECTOR_OPERATOR);
        pool.upd(updateAnchorPrice, updateFeeAskX24, updateFeeBidX24);

        (update.anchorPriceAfter, update.feeAskX24After, update.feeBidX24After,) = pool.state();
        update.reserveXAfter = pool.getXReserve();
        update.reserveYAfter = pool.getYReserve();
        update.maxPunishmentX24After = pool.maxPunishmentX24();

        assertEq(update.anchorPriceAfter, updateAnchorPrice, "upd did not replace anchor");
        assertEq(update.feeAskX24After, updateFeeAskX24, "upd did not replace ask fee");
        assertEq(update.feeBidX24After, updateFeeBidX24, "upd did not replace bid fee");
        assertEq(update.reserveXAfter, reserveXBefore, "upd changed X reserve");
        assertEq(update.reserveYAfter, reserveYBefore, "upd changed Y reserve");
        assertEq(update.maxPunishmentX24After, expectedMaxPunishmentX24, "upd changed max punishment");
    }

    function _assertPunishmentMathRevert(VectorInput memory input) private view {
        try punishmentHarness.punishmentX24(
            input.anchorPrice, input.amountIn, input.reserveX, input.reserveY, input.maxPunishmentX24, input.xToY
        ) returns (
            uint24
        ) {
            revert("swap panic was not punishment mulDiv");
        } catch (bytes memory reason) {
            _assertMathMulDivRevert(reason);
        }
    }

    function _selector(bytes memory reason) private pure returns (bytes4 selector) {
        if (reason.length < 4) return bytes4(0);
        assembly ("memory-safe") {
            selector := mload(add(reason, 0x20))
        }
    }

    function _assertExactSelector(bytes memory reason, bytes4 expected, uint256 expectedLength) private pure {
        assertEq(reason.length, expectedLength, "unexpected revert-data length");
        assertEq(_selector(reason), expected, "unexpected revert selector");
    }

    function _assertReserveTransitionRevert(bytes memory reason) private pure {
        _assertExactSelector(reason, SAFE_CAST_OVERFLOW_SELECTOR, 68);
        uint256 bits;
        assembly ("memory-safe") {
            bits := mload(add(reason, 0x24))
        }
        assertEq(bits, 112, "unexpected SafeCast target width");
    }

    function _isMathMulDivRevert(bytes memory reason) private pure returns (bool) {
        if (reason.length != 36 || _selector(reason) != PANIC_SELECTOR) return false;
        uint256 panicCode;
        assembly ("memory-safe") {
            panicCode := mload(add(reason, 0x24))
        }
        return panicCode == PANIC_ARITHMETIC_UNDER_OVERFLOW;
    }

    function _assertMathMulDivRevert(bytes memory reason) private pure {
        assertTrue(_isMathMulDivRevert(reason), "expected Panic(0x11) mulDiv revert");
    }

    function _revertWith(bytes memory reason) private pure {
        assembly ("memory-safe") {
            revert(add(reason, 0x20), mload(reason))
        }
    }

    function _serializeVector(
        string memory identityKey,
        string memory identityValue,
        VectorInput memory input,
        VectorOutput memory output,
        UpdateOutput memory update
    ) private pure returns (string memory json) {
        json = string.concat('{"schemaVersion":4,"', identityKey, '":"', identityValue, '","dir":"');
        json = string.concat(json, input.xToY ? "xToY" : "yToX", '"');
        json = string.concat(json, _serializeInputA(input), _serializeInputB(input));
        json = string.concat(json, _serializeQuote(output), _serializeTransitionA(output));
        json = string.concat(json, _serializeTransitionB(output));
        if (update.enabled) json = string.concat(json, _serializeUpdate(update));
        return string.concat(json, "}");
    }

    function _serializeInputA(VectorInput memory input) private pure returns (string memory) {
        return string.concat(
            _uintField("anchorPrice", input.anchorPrice),
            _uintField("feeAskX24", input.feeAskX24),
            _uintField("feeBidX24", input.feeBidX24),
            _uintField("reserveX", input.reserveX)
        );
    }

    function _serializeInputB(VectorInput memory input) private pure returns (string memory) {
        return string.concat(
            _uintField("reserveY", input.reserveY),
            _uintField("maxPunishmentX24", input.maxPunishmentX24),
            _uintField("feeMultiplier", input.feeMultiplier),
            _uintField("amountIn", input.amountIn)
        );
    }

    function _serializeQuote(VectorOutput memory output) private pure returns (string memory json) {
        json = string.concat(
            _uintField("amountOut", output.amountOut),
            _uintField("pNext", output.pNext),
            _uintField("feeAmount", output.feeAmount)
        );
        return string.concat(
            json,
            ',"outcome":"',
            _outcomeString(output.outcome),
            '","revertSelector":"',
            _revertSelectorString(output.outcome),
            '","revertClass":"',
            _revertClassString(output.outcome),
            '"'
        );
    }

    function _serializeTransitionA(VectorOutput memory output) private pure returns (string memory) {
        return string.concat(
            _uintField("desiredPunishmentX24", output.desiredPunishmentX24),
            _uintField("effectiveFeeX24", output.effectiveFeeX24),
            _uintField("appliedPunishmentX24", output.appliedPunishmentX24),
            _uintField("feeAskX24After", output.feeAskX24After),
            _uintField("feeBidX24After", output.feeBidX24After)
        );
    }

    function _serializeTransitionB(VectorOutput memory output) private pure returns (string memory) {
        return string.concat(
            _uintField("reserveXAfter", output.reserveXAfter), _uintField("reserveYAfter", output.reserveYAfter)
        );
    }

    function _serializeUpdate(UpdateOutput memory update) private pure returns (string memory json) {
        json = string.concat(',"update":{"anchorPrice":"', vm.toString(update.anchorPrice), '"');
        json = string.concat(
            json,
            _uintField("feeAskX24", update.feeAskX24),
            _uintField("feeBidX24", update.feeBidX24),
            _uintField("anchorPriceAfter", update.anchorPriceAfter)
        );
        json = string.concat(
            json,
            _uintField("feeAskX24After", update.feeAskX24After),
            _uintField("feeBidX24After", update.feeBidX24After),
            _uintField("reserveXAfter", update.reserveXAfter)
        );
        return string.concat(
            json,
            _uintField("reserveYAfter", update.reserveYAfter),
            _uintField("maxPunishmentX24After", update.maxPunishmentX24After),
            "}"
        );
    }

    function _outcomeString(VectorOutcome outcome) private pure returns (string memory) {
        if (outcome == VectorOutcome.Applied) return "Applied";
        if (outcome == VectorOutcome.SwapImpossible) return "SwapImpossible";
        if (outcome == VectorOutcome.ReserveTransitionOverflow) return "ReserveTransitionOverflow";
        return "MathMulDivRevert";
    }

    function _revertSelectorString(VectorOutcome outcome) private pure returns (string memory) {
        if (outcome == VectorOutcome.Applied) return "0x00000000";
        if (outcome == VectorOutcome.SwapImpossible) return "0x4a45e749";
        if (outcome == VectorOutcome.ReserveTransitionOverflow) return "0x6dfcc650";
        return "0x4e487b71";
    }

    function _revertClassString(VectorOutcome outcome) private pure returns (string memory) {
        if (outcome == VectorOutcome.Applied) return "None";
        if (outcome == VectorOutcome.SwapImpossible) return "SwapImpossible()";
        if (outcome == VectorOutcome.ReserveTransitionOverflow) {
            return "SafeCastOverflowedUintDowncast(uint8,uint256)";
        }
        return "Panic(0x11)";
    }

    function _uintField(string memory key, uint256 value) private pure returns (string memory) {
        return string.concat(',"', key, '":"', vm.toString(value), '"');
    }
}
