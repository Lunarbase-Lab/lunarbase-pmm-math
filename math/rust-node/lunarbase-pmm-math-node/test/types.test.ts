import {
  SwapSimulationStatus,
  type SwapSimulationResult,
} from "@lunarbase-lab/pmm-math";

declare const simulation: SwapSimulationResult;

const status: SwapSimulationStatus = simulation.status;
const committed: boolean = status === SwapSimulationStatus.Applied;

void committed;
