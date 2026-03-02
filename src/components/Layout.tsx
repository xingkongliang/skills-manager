import { Outlet } from "react-router-dom";
import { Sidebar } from "./Sidebar";
import { StatusBanner } from "./StatusBanner";
import { useApp } from "../context/AppContext";
import { useTranslation } from "react-i18next";

export function Layout() {
  const { t } = useTranslation();
  const { appError, refreshAppData } = useApp();

  return (
    <div className="flex h-screen w-full overflow-hidden bg-background text-primary">
      <Sidebar />
      <div className="relative flex min-w-[600px] flex-1 flex-col overflow-hidden">
        <div className="relative z-0 flex-1 overflow-y-auto p-5 scrollbar-hide">
          <div className="mx-auto flex h-full max-w-[1200px] flex-col gap-4">
            {appError ? (
              <StatusBanner
                compact
                title={t("common.dataOutOfDate")}
                description={appError}
                actionLabel={t("common.retry")}
                onAction={refreshAppData}
                tone="danger"
              />
            ) : null}
            <Outlet />
          </div>
        </div>
      </div>
    </div>
  );
}
