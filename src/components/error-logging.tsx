"use client";

import { useEffect } from "react";
import { installErrorLogging } from "@/lib/error-log";

/** Sends this window's uncaught errors to the log file. Renders nothing. */
export function ErrorLogging() {
  useEffect(installErrorLogging, []);
  return null;
}
