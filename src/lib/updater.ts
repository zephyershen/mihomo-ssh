import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";

const hasTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export type AppUpdateStatus =
  | {
      state: "unavailable";
      message: string;
    }
  | {
      state: "current";
      message: string;
    }
  | {
      state: "available";
      version: string;
      date?: string;
      body?: string;
    };

export type AppUpdateProgress = {
  phase: "idle" | "checking" | "downloading" | "installing" | "restarting" | "error";
  message: string;
  downloaded?: number;
  total?: number;
};

export async function checkForAppUpdate(): Promise<AppUpdateStatus> {
  if (!hasTauri) {
    return {
      state: "unavailable",
      message: "浏览器预览模式无法检查桌面更新。",
    };
  }

  try {
    const update = await check();
    if (!update) {
      return { state: "current", message: "当前已是最新版本。" };
    }

    return {
      state: "available",
      version: update.version,
      date: update.date ? String(update.date) : undefined,
      body: update.body || undefined,
    };
  } catch (error) {
    return {
      state: "unavailable",
      message: normalizeUpdateError(error),
    };
  }
}

export async function downloadInstallAndRelaunch(
  onProgress: (progress: AppUpdateProgress) => void,
): Promise<void> {
  if (!hasTauri) {
    throw new Error("浏览器预览模式无法安装桌面更新。");
  }

  onProgress({ phase: "checking", message: "正在检查更新" });
  const update = await check();
  if (!update) {
    onProgress({ phase: "idle", message: "当前已是最新版本" });
    return;
  }

  let downloaded = 0;
  let total: number | undefined;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength || undefined;
        downloaded = 0;
        onProgress({
          phase: "downloading",
          message: "开始下载更新",
          downloaded,
          total,
        });
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress({
          phase: "downloading",
          message: "正在下载更新",
          downloaded,
          total,
        });
        break;
      case "Finished":
        onProgress({ phase: "installing", message: "正在安装更新", downloaded, total });
        break;
    }
  });

  onProgress({ phase: "restarting", message: "更新已安装，正在重启应用" });
  await relaunch();
}

function normalizeUpdateError(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  if (/pubkey|endpoint|config|update|plugin|not found/i.test(raw)) {
    return "当前安装包未启用更新源。请先安装由 GitHub Release 发布的版本。";
  }
  return raw;
}
