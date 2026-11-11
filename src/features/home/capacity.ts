/**
 * Whether this machine has too little RAM to run labs comfortably: under 8 GB. Container labs plus
 * Docker Desktop and a full Exegol image want headroom; multi-VM labs want more. Home says so, and
 * points to a cloud account or a server instead.
 */
export const isLowRam = (memTotalBytes: number): boolean => memTotalBytes < 8e9;
