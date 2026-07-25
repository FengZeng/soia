import { createApp } from "vue";
import { coreClientKey } from "../core-client/coreClientKey";
import RemoteApp from "./RemoteApp.vue";
import { remoteCoreClient } from "./remoteCoreClient";
import "./remote.css";

createApp(RemoteApp)
    .provide(coreClientKey, remoteCoreClient)
    .mount("#app");
