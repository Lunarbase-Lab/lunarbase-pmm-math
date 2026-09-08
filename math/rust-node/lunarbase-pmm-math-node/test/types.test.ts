import {
  buildOrderBook,
  buildValidatedOrderBook,
  buildPreciseOrderBook,
  validateFeeAccountingCapacity,
  ladderAmountOut,
  OrderBookSafety,
  OrderBookStatus,
  SwapSimulationStatus,
  type BuildOrderBookParams,
  type OrderBookResult,
  type OrderBookSnapshot,
  type ValidatedOrderBookConfig,
  type ValidatedOrderBookResult,
  type OrderBookPrecision,
  type PreciseOrderBookResult,
  type FeeAccountingState,
  type SwapSimulationResult,
} from "@lunarbase-lab/pmm-math";

declare const simulation: SwapSimulationResult;

const status: SwapSimulationStatus = simulation.status;
const committed: boolean = status === SwapSimulationStatus.Applied;

void committed;

declare const orderBookParams: BuildOrderBookParams;
const orderBook: OrderBookResult = buildOrderBook(orderBookParams);
const publishable: boolean = orderBook.status === OrderBookStatus.Active;
const firstPrice: string | undefined = orderBook.xToY.levels[0]?.price;

void publishable;
void firstPrice;

declare const snapshot: OrderBookSnapshot;
declare const policy: ValidatedOrderBookConfig;
declare const accounting: FeeAccountingState;
validateFeeAccountingCapacity(snapshot, policy, accounting);
const validated: ValidatedOrderBookResult = buildValidatedOrderBook(snapshot, policy);
const certified: boolean = validated.book.safety === OrderBookSafety.ExhaustiveLotPolicy;
const swept: string | null = ladderAmountOut(validated.book.xToY.levels, "10", "20");
void certified;
void swept;

const precision = { maxLevels: 4, targetUnderquoteBps: 10, maxWork: 1_000_000 } satisfies OrderBookPrecision;
const precise: PreciseOrderBookResult = buildPreciseOrderBook(snapshot, policy, precision);
const meetsWorstStateTarget: boolean = precise.targetMet;
const worstStateGap: number = precise.worstUnderquoteBps;
const freshSnapshotGap: number = precise.worstFreshSnapshotDiscountBps;
const preciseBook: OrderBookResult = precise.validated.book;
void meetsWorstStateTarget;
void worstStateGap;
void freshSnapshotGap;
void preciseBook;
