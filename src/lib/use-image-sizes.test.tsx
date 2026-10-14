// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { imageDownloadSize } = vi.hoisted(() => ({ imageDownloadSize: vi.fn<(image: string) => Promise<number | null>>() }));
vi.mock("@/lib/tauri", () => ({ imageDownloadSize }));

const { useImageSizes } = await import("@/lib/use-image-sizes");

// The cache is per module, so each test asks for images of its own.
describe("useImageSizes", () => {
  it("asks Docker Hub once per image, sharing the answer between hooks", async () => {
    imageDownloadSize.mockResolvedValue(123);
    const a = renderHook(() => useImageSizes(["img-a"]));
    const b = renderHook(() => useImageSizes(["img-a"]));
    await act(async () => {});
    expect(imageDownloadSize).toHaveBeenCalledTimes(1);
    expect(a.result.current).toEqual({ "img-a": 123 });
    expect(b.result.current).toEqual({ "img-a": 123 });
  });

  it("remembers a failed lookup as unknown and does not ask again", async () => {
    imageDownloadSize.mockRejectedValueOnce(new Error("hub down"));
    const first = renderHook(() => useImageSizes(["img-b"]));
    await act(async () => {});
    expect(first.result.current).toEqual({ "img-b": null });
    const calls = imageDownloadSize.mock.calls.length;
    renderHook(() => useImageSizes(["img-b"]));
    expect(imageDownloadSize).toHaveBeenCalledTimes(calls);
  });

  it("loads the sizes of images added later and keeps the earlier ones", async () => {
    imageDownloadSize.mockImplementation(async (image) => image.length);
    const { result, rerender } = renderHook(({ images }) => useImageSizes(images), { initialProps: { images: ["img-c"] } });
    await act(async () => {});
    rerender({ images: ["img-c", "img-dddd"] });
    await act(async () => {});
    expect(result.current).toEqual({ "img-c": 5, "img-dddd": 8 });
  });

  it("asks nothing for an empty list or blank names", () => {
    const calls = imageDownloadSize.mock.calls.length;
    renderHook(() => useImageSizes([]));
    renderHook(() => useImageSizes([""]));
    expect(imageDownloadSize).toHaveBeenCalledTimes(calls);
  });
});
