import { type ComponentType } from "react";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import { AnimatePresence, motion } from "framer-motion";
import {
  ArrowUpRight,
  BarChart2,
  BookOpen,
  Cable,
  type LucideProps,
  LayoutDashboard,
  Settings as SettingsIcon,
  Target,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Logo } from "@/components/Logo";
import { Badge } from "@/components/ui/badge";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useLoggingState } from "@/hooks/useLoggingState";
import { useDevice } from "@/hooks/useDevice";

interface NavItem {
  to: string;
  label: string;
  icon: ComponentType<LucideProps>;
}

const NAV: NavItem[] = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard },
  { to: "/analytics", label: "Analytics", icon: BarChart2 },
  { to: "/suggestions", label: "Words", icon: BookOpen },
  { to: "/proficiency", label: "Proficiency", icon: Target },
  { to: "/device", label: "Device", icon: Cable },
  { to: "/settings", label: "Settings", icon: SettingsIcon },
];

function SidebarLink({ item }: { item: NavItem }) {
  const Icon = item.icon;
  return (
    <NavLink
      to={item.to}
      end={item.to === "/"}
      className="rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring/60 focus-visible:ring-offset-2 focus-visible:ring-offset-sidebar"
    >
      {({ isActive }) => (
        <div
          className={cn(
            "group relative flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors duration-200",
            isActive
              ? "text-foreground"
              : "text-muted-foreground hover:bg-sidebar-accent/50 hover:text-foreground",
          )}
        >
          {isActive && (
            <motion.div
              layoutId="nav-active"
              transition={{ type: "spring", stiffness: 420, damping: 34 }}
              className="absolute inset-0 rounded-lg bg-sidebar-accent ring-1 ring-foreground/5"
            />
          )}
          <Icon
            className={cn(
              "relative z-10 size-[18px] shrink-0 transition-colors",
              isActive ? "text-gold" : "text-current",
            )}
            strokeWidth={1.85}
          />
          <span className="relative z-10">{item.label}</span>
        </div>
      )}
    </NavLink>
  );
}

function LoggingPill() {
  const { state, busy, toggle } = useLoggingState();
  const active = state.logging;
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            onClick={() => void toggle()}
            disabled={busy}
            aria-label={active ? "Pause logging" : "Resume logging"}
            className={cn(
              "inline-flex items-center gap-2 rounded-full border py-1.5 pr-3.5 pl-2.5 text-xs font-medium transition-colors outline-none",
              "focus-visible:ring-3 focus-visible:ring-ring/50",
              active
                ? "border-success/30 bg-success/10 text-success hover:bg-success/15"
                : "border-border bg-secondary text-muted-foreground hover:text-foreground",
              busy && "opacity-60",
            )}
          >
            <span
              className={cn(
                "size-2 rounded-full",
                active ? "bg-success animate-pulse-soft" : "bg-muted-foreground",
              )}
            />
            {active ? "Logging" : "Paused"}
          </button>
        </TooltipTrigger>
        <TooltipContent side="bottom">
          {active ? "Click to pause keystroke logging" : "Click to resume logging"}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

function DeviceChip() {
  const { device } = useDevice();
  return (
    <Badge
      variant="outline"
      className="gap-1.5 py-1 pr-2.5 pl-2 text-xs font-medium"
    >
      <Cable
        className={cn(
          "size-3.5",
          device ? "text-gold" : "text-muted-foreground/60",
        )}
      />
      <span className={device ? "text-foreground" : "text-muted-foreground"}>
        {device ? device.name || device.device : "No device"}
      </span>
    </Badge>
  );
}

export function AppShell() {
  const location = useLocation();
  return (
    <div className="app-canvas relative flex h-full min-h-0 w-full overflow-hidden">
      {/*
        Draggable strip for the frameless (Overlay) macOS title bar. Scoped to the
        sidebar column (left of the header controls) so it never covers the
        interactive Device/Logging chips on the right. The header carries its own
        drag region across its empty areas.
      */}
      <div
        data-tauri-drag-region
        className="absolute top-0 left-0 z-50 h-7 w-[232px]"
        aria-hidden
      />

      {/* Sidebar */}
      <aside className="flex w-[232px] shrink-0 flex-col border-r border-border bg-sidebar/70 backdrop-blur-sm">
        {/* Top padding clears the floating macOS traffic lights (~x:20 y:20). */}
        <div data-tauri-drag-region className="px-5 pt-10 pb-2">
          <Logo />
        </div>
        <p className="px-5 pb-5 text-[11px] tracking-wide text-muted-foreground/70">
          Chord. Measure. Master.
        </p>
        <nav className="flex flex-1 flex-col gap-1 px-3">
          {NAV.map((item) => (
            <SidebarLink key={item.to} item={item} />
          ))}
        </nav>
        <div className="px-5 pb-5 pt-3">
          <button
            type="button"
            onClick={() =>
              void openUrl("https://github.com/natebakescakes/cadenza/releases")
            }
            aria-label="View releases on GitHub"
            className="group inline-flex items-center gap-1.5 rounded-md font-mono text-[10px] tracking-wide text-muted-foreground/50 outline-none transition-colors hover:text-gold focus-visible:ring-2 focus-visible:ring-ring/60 focus-visible:ring-offset-2 focus-visible:ring-offset-sidebar"
          >
            <span>v0.0.1-alpha</span>
            <ArrowUpRight className="size-2.5 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
          </button>
        </div>
      </aside>

      {/* Main column */}
      <div className="flex min-w-0 flex-1 flex-col">
        {/* Top bar — pt clears the floating traffic-light drag strip. */}
        <header
          data-tauri-drag-region
          className="flex h-[60px] shrink-0 items-center justify-between gap-3 border-b border-border bg-background/40 px-6 pt-1.5 backdrop-blur-sm"
        >
          <div className="flex items-center gap-2 text-xs text-muted-foreground/70">
            <span className="hidden sm:inline">Premium analytics for chorded typing</span>
          </div>
          <div className="flex items-center gap-2.5">
            <DeviceChip />
            <LoggingPill />
          </div>
        </header>

        {/* Animated routed content */}
        <main className="min-h-0 flex-1 overflow-y-auto">
          <AnimatePresence mode="wait">
            <motion.div
              key={location.pathname}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -6 }}
              transition={{ duration: 0.28, ease: [0.16, 1, 0.3, 1] }}
              className="mx-auto w-full max-w-6xl px-6 py-8 sm:px-8"
            >
              <Outlet />
            </motion.div>
          </AnimatePresence>
        </main>
      </div>
    </div>
  );
}
