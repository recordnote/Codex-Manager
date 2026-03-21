"use client";

import { useEffect, useState } from "react";
import { isTauriRuntime } from "@/lib/api/transport";

export function useDeferredDesktopActivation(enabled: boolean): boolean {
  const shouldDefer = isTauriRuntime();
  const [isActivated, setIsActivated] = useState(
    () => enabled && !shouldDefer,
  );

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    if (!enabled) {
      const frameId = window.requestAnimationFrame(() => {
        setIsActivated(false);
      });
      return () => {
        window.cancelAnimationFrame(frameId);
      };
    }

    if (!shouldDefer) {
      const frameId = window.requestAnimationFrame(() => {
        setIsActivated(true);
      });
      return () => {
        window.cancelAnimationFrame(frameId);
      };
    }

    let cancelled = false;
    const resetFrameId = window.requestAnimationFrame(() => {
      setIsActivated(false);
    });
    let secondFrameId: number | null = null;
    const firstFrameId = window.requestAnimationFrame(() => {
      secondFrameId = window.requestAnimationFrame(() => {
        if (!cancelled) {
          setIsActivated(true);
        }
      });
    });

    return () => {
      cancelled = true;
      window.cancelAnimationFrame(resetFrameId);
      window.cancelAnimationFrame(firstFrameId);
      if (secondFrameId !== null) {
        window.cancelAnimationFrame(secondFrameId);
      }
    };
  }, [enabled, shouldDefer]);

  return enabled ? isActivated : false;
}
