import type { CommandResultDto } from "./generated/CommandResultDto";
import type { CoreErrorDto } from "./generated/CoreErrorDto";
import type { PlaybackCommandDto } from "./generated/PlaybackCommandDto";
import type { PlaybackSnapshotDto } from "./generated/PlaybackSnapshotDto";

export type CoreClientError =
    | {
          type: "core";
          error: CoreErrorDto;
      }
    | {
          type: "transport";
          message: string;
      }
    | {
          type: "protocol";
          message: string;
      };

export type CoreClientSnapshotListener = (
    snapshot: PlaybackSnapshotDto,
) => void;

export type CoreClientErrorListener = (error: CoreClientError) => void;

export type CoreClientUnsubscribe = () => void;

/**
 * Transport-neutral playback contract shared by Desktop, Browser, and future
 * clients. Implementations own connection lifecycle and must surface failures
 * as CoreClientError values rather than transport-specific exceptions.
 */
export interface CoreClient {
    /** Returns the latest authoritative snapshot available from Core. */
    getSnapshot(): Promise<PlaybackSnapshotDto>;

    /**
     * Starts a snapshot subscription and returns a synchronous disposer. After
     * disposal, an implementation must not invoke either listener again.
     */
    subscribe(
        listener: CoreClientSnapshotListener,
        onError?: CoreClientErrorListener,
    ): CoreClientUnsubscribe;

    /**
     * Executes one typed playback command. Transport retries for this call must
     * reuse its original command ID so Core can deduplicate the request.
     */
    execute(command: PlaybackCommandDto): Promise<CommandResultDto>;
}
