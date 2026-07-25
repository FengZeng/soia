import type { InjectionKey } from "vue";
import type { CoreClient } from "./CoreClient";

export const coreClientKey: InjectionKey<CoreClient> = Symbol("core-client");
