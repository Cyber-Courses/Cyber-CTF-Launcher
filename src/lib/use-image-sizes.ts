"use client";

import { useEffect, useState } from "react";
import { imageDownloadSize } from "@/lib/tauri";

// Sizes don't change while the app is open: ask Docker Hub once per image.
const cache = new Map<string, Promise<number | null>>();

/** An image's download size, asked once (null when Docker Hub doesn't say). */
function sizeOf(image: string): Promise<number | null> {
  const known = cache.get(image);
  if (known) return known;
  const asked = imageDownloadSize(image).catch(() => null);
  cache.set(image, asked);
  return asked;
}

/** Download size per image (bytes; null when Docker Hub doesn't say, undefined while loading). */
export function useImageSizes(images: string[]): Record<string, number | null | undefined> {
  const [sizes, setSizes] = useState<Record<string, number | null>>({});
  const key = images.join("|");
  useEffect(() => {
    let live = true;
    for (const image of key.split("|").filter(Boolean)) {
      void sizeOf(image).then((size) => {
        if (live) setSizes((s) => ({ ...s, [image]: size }));
      });
    }
    return () => {
      live = false;
    };
  }, [key]);
  return sizes;
}
