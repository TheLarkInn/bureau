import budget from "../windows/native-budget.json" with { type: "json" };

import { requireValue } from "../../scripts/maintenance-contract.mjs";
import { exactKeys } from "./protocol.mjs";

exactKeys(budget, ["schema", "engineMemoryMaxBytes", "guardianMemoryMaxBytes"]);
requireValue(budget.schema === "bureau-native-budget-v1"
  && Number.isSafeInteger(budget.engineMemoryMaxBytes) && budget.engineMemoryMaxBytes > 0
  && budget.engineMemoryMaxBytes <= 8 * 1024 ** 3
  && Number.isSafeInteger(budget.guardianMemoryMaxBytes) && budget.guardianMemoryMaxBytes > 0
  && budget.guardianMemoryMaxBytes <= 128 * 1024 ** 2, "invalid reviewed native memory allocation");

export const NATIVE_BUDGET = Object.freeze(budget);
