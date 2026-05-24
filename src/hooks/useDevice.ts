import { useCallback, useEffect, useState } from "react";
import {
  connectDevice,
  currentDevice,
  onDeviceChanged,
  refreshChordmap,
  scanDevices,
} from "@/lib/api";
import type { DeviceInfo, SerialPortInfo } from "@/lib/types";

export function useDevice() {
  const [device, setDevice] = useState<DeviceInfo | null>(null);
  const [ports, setPorts] = useState<SerialPortInfo[]>([]);
  const [scanning, setScanning] = useState(false);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Initial device + live device-changed events.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    currentDevice()
      .then(setDevice)
      .catch(() => setDevice(null));
    onDeviceChanged(setDevice)
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  const scan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      setPorts(await scanDevices());
    } catch {
      setPorts([]);
      setError("Could not scan for devices.");
    } finally {
      setScanning(false);
    }
  }, []);

  const connect = useCallback(async (port: string) => {
    setConnecting(port);
    setError(null);
    try {
      const info = await connectDevice(port);
      setDevice(info);
      return info;
    } catch {
      setError("Failed to connect to device.");
      return null;
    } finally {
      setConnecting(null);
    }
  }, []);

  const refresh = useCallback(async () => {
    try {
      return await refreshChordmap();
    } catch {
      setError("Failed to refresh chord map.");
      return 0;
    }
  }, []);

  return {
    device,
    ports,
    scanning,
    connecting,
    error,
    scan,
    connect,
    refreshMap: refresh,
  };
}
