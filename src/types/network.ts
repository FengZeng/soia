export type NetworkConnectionStatus = "Idle" | "Connected" | "Error";

export type NetworkConnection = {
    id: string;
    label: string;
    protocol: string;
    baseUrl: string;
    username: string;
    password: string;
    defaultPath: string;
    tlsCertificateDer?: string | null;
};

export type NetworkBrowseEntry = {
    name: string;
    path: string;
    entryType: "dir" | "file";
    playbackKey?: string | null;
    size: number | null;
    modifiedAt: string | null;
    createdAt: string | null;
};

export type NetworkBrowseResult = {
    path: string;
    entries: NetworkBrowseEntry[];
};

export type NetworkFileRow = {
    name: string;
    path: string;
    type: "DIR" | "FILE";
    playbackKey?: string;
    size: string;
    modified: string;
    modifiedAt?: string | null;
    createdAt?: string | null;
    isParent?: boolean;
    playbackProgressText?: string;
    isActive?: boolean;
    containsActive?: boolean;
};

export type NetworkPlayRequest = {
    playbackKey: string;
    displayName: string;
};
