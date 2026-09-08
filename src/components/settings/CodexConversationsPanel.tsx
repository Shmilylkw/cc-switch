import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  Loader2,
  RefreshCw,
  Trash2,
  MessageSquare,
  Database,
  Clock,
  FolderOpen,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { useSessionsQuery } from "@/lib/query";
import { usageKeys } from "@/lib/query/usage";
import { sessionsApi, usageApi } from "@/lib/api";
import type { SessionMeta } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  formatSessionTitle,
  formatTimestamp,
  getBaseName,
  getSessionKey,
} from "@/components/sessions/utils";

export function CodexConversationsPanel() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data, isLoading, isFetching, refetch } = useSessionsQuery();

  const conversations = useMemo(
    () => (data ?? []).filter((session) => session.providerId === "codex"),
    [data],
  );
  const deletable = useMemo(
    () => conversations.filter((session) => Boolean(session.sourcePath)),
    [conversations],
  );

  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(
    () => new Set(),
  );
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [showRebuildConfirm, setShowRebuildConfirm] = useState(false);
  const [isRebuilding, setIsRebuilding] = useState(false);

  // 会话列表刷新后，剔除已不存在的选中项
  useEffect(() => {
    const validKeys = new Set(conversations.map((s) => getSessionKey(s)));
    setSelectedKeys((current) => {
      let changed = false;
      const next = new Set<string>();
      current.forEach((key) => {
        if (validKeys.has(key)) next.add(key);
        else changed = true;
      });
      return changed ? next : current;
    });
  }, [conversations]);

  const selectedSessions = useMemo(
    () => deletable.filter((session) => selectedKeys.has(getSessionKey(session))),
    [deletable, selectedKeys],
  );

  const allSelected =
    deletable.length > 0 &&
    deletable.every((session) => selectedKeys.has(getSessionKey(session)));

  const toggleOne = (session: SessionMeta, checked: boolean) => {
    if (!session.sourcePath) return;
    const key = getSessionKey(session);
    setSelectedKeys((current) => {
      const next = new Set(current);
      if (checked) next.add(key);
      else next.delete(key);
      return next;
    });
  };

  const toggleAll = () => {
    setSelectedKeys((current) => {
      const next = new Set(current);
      if (allSelected) {
        deletable.forEach((session) => next.delete(getSessionKey(session)));
      } else {
        deletable.forEach((session) => next.add(getSessionKey(session)));
      }
      return next;
    });
  };

  const handleDeleteConfirm = async () => {
    const targets = selectedSessions.filter((session) => session.sourcePath);
    setShowDeleteConfirm(false);
    if (targets.length === 0) return;

    setIsDeleting(true);
    try {
      const results = await sessionsApi.deleteMany(
        targets.map((session) => ({
          providerId: session.providerId,
          sessionId: session.sessionId,
          sourcePath: session.sourcePath!,
        })),
      );

      const deletedKeys = results
        .filter((result) => result.success)
        .map(
          (result) =>
            `${result.providerId}:${result.sessionId}:${result.sourcePath ?? ""}`,
        );
      const failedCount = results.filter((result) => !result.success).length;

      if (deletedKeys.length > 0) {
        const deletedKeySet = new Set(deletedKeys);
        queryClient.setQueryData<SessionMeta[]>(["sessions"], (current) =>
          (current ?? []).filter(
            (session) => !deletedKeySet.has(getSessionKey(session)),
          ),
        );
        setSelectedKeys((current) => {
          const next = new Set(current);
          deletedKeys.forEach((key) => next.delete(key));
          return next;
        });
        await queryClient.invalidateQueries({ queryKey: ["sessions"] });
        toast.success(
          t("settings.advanced.codexConversations.deleteSuccess", {
            count: deletedKeys.length,
          }),
        );
      }

      if (failedCount > 0) {
        toast.error(
          t("settings.advanced.codexConversations.deleteFailed", {
            failed: failedCount,
          }),
        );
      }
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("settings.advanced.codexConversations.deleteRequestFailed"),
      );
    } finally {
      setIsDeleting(false);
    }
  };

  const handleRebuildConfirm = async () => {
    setShowRebuildConfirm(false);
    setIsRebuilding(true);
    try {
      const result = await usageApi.rebuildCodexUsage();
      await queryClient.invalidateQueries({ queryKey: usageKeys.all });
      const message = t("settings.advanced.codexConversations.rebuildSuccess", {
        imported: result.imported,
        errors: result.errors.length,
      });
      if (result.errors.length > 0 || result.deferredFiles > 0) {
        toast.warning(message);
      } else {
        toast.success(message);
      }
    } catch (error) {
      toast.error(
        t("settings.advanced.codexConversations.rebuildFailed", {
          error: extractErrorMessage(error) || String(error),
        }),
      );
    } finally {
      setIsRebuilding(false);
    }
  };

  return (
    <div className="space-y-6">
      {/* 本地对话列表 */}
      <div className="space-y-3">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium">
              {t("settings.advanced.codexConversations.listTitle")}
            </span>
            <Badge variant="secondary" className="text-xs">
              {conversations.length}
            </Badge>
          </div>
          <Button
            variant="ghost"
            size="sm"
            className="h-8 gap-1.5"
            onClick={() => void refetch()}
            disabled={isFetching}
          >
            <RefreshCw
              className={`size-3.5 ${isFetching ? "animate-spin" : ""}`}
            />
            <span className="text-xs">
              {t("settings.advanced.codexConversations.refresh")}
            </span>
          </Button>
        </div>

        {/* 批量操作工具栏 */}
        {deletable.length > 0 && (
          <div className="flex flex-wrap items-center gap-3 rounded-md border bg-muted/40 px-3 py-2">
            <label className="flex cursor-pointer items-center gap-2 text-xs text-muted-foreground">
              <Checkbox
                checked={
                  allSelected
                    ? true
                    : selectedSessions.length > 0
                      ? "indeterminate"
                      : false
                }
                onCheckedChange={toggleAll}
                aria-label={t(
                  "settings.advanced.codexConversations.selectAll",
                )}
              />
              <span>{t("settings.advanced.codexConversations.selectAll")}</span>
            </label>
            <Badge variant="outline" className="text-xs">
              {t("settings.advanced.codexConversations.selectedCount", {
                count: selectedSessions.length,
              })}
            </Badge>
            <Button
              variant="destructive"
              size="sm"
              className="ml-auto h-7 gap-1.5"
              onClick={() => setShowDeleteConfirm(true)}
              disabled={isDeleting || selectedSessions.length === 0}
            >
              {isDeleting ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <Trash2 className="size-3.5" />
              )}
              <span className="text-xs">
                {isDeleting
                  ? t("settings.advanced.codexConversations.deleting")
                  : t("settings.advanced.codexConversations.delete")}
              </span>
            </Button>
          </div>
        )}

        {/* 列表主体 */}
        {isLoading ? (
          <div className="flex items-center justify-center py-10">
            <Loader2 className="size-5 animate-spin text-muted-foreground" />
          </div>
        ) : conversations.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-2 py-10 text-center">
            <MessageSquare className="size-8 text-muted-foreground/50" />
            <p className="text-sm text-muted-foreground">
              {t("settings.advanced.codexConversations.empty")}
            </p>
          </div>
        ) : (
          <div className="max-h-72 space-y-1 overflow-y-auto pr-1">
            {conversations.map((session) => {
              const key = getSessionKey(session);
              const disabled = !session.sourcePath;
              return (
                <label
                  key={key}
                  className={`flex items-start gap-3 rounded-md border border-transparent px-2.5 py-2 transition-colors hover:bg-muted/60 ${
                    disabled ? "opacity-60" : "cursor-pointer"
                  }`}
                >
                  <Checkbox
                    className="mt-0.5"
                    checked={selectedKeys.has(key)}
                    disabled={disabled}
                    onCheckedChange={(checked) =>
                      toggleOne(session, checked === true)
                    }
                    aria-label={formatSessionTitle(session)}
                  />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium">
                      {formatSessionTitle(session)}
                    </p>
                    <div className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
                      <span className="flex items-center gap-1">
                        <Clock className="size-3" />
                        {formatTimestamp(
                          session.lastActiveAt ?? session.createdAt,
                        )}
                      </span>
                      {session.projectDir && (
                        <span className="flex items-center gap-1 truncate">
                          <FolderOpen className="size-3 shrink-0" />
                          <span className="truncate">
                            {getBaseName(session.projectDir)}
                          </span>
                        </span>
                      )}
                    </div>
                  </div>
                </label>
              );
            })}
          </div>
        )}
      </div>

      {/* 刷新数据库 */}
      <div className="space-y-3 border-t border-border/50 pt-5">
        <div className="flex items-start gap-3">
          <Database className="mt-0.5 size-5 shrink-0 text-blue-500" />
          <div className="space-y-1">
            <p className="text-sm font-medium">
              {t("settings.advanced.codexConversations.rebuildTitle")}
            </p>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.codexConversations.rebuildDescription")}
            </p>
          </div>
        </div>
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5"
          onClick={() => setShowRebuildConfirm(true)}
          disabled={isRebuilding}
        >
          {isRebuilding ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <RefreshCw className="size-3.5" />
          )}
          <span className="text-xs">
            {isRebuilding
              ? t("settings.advanced.codexConversations.rebuilding")
              : t("settings.advanced.codexConversations.rebuild")}
          </span>
        </Button>
      </div>

      <ConfirmDialog
        isOpen={showDeleteConfirm}
        title={t("settings.advanced.codexConversations.deleteConfirmTitle")}
        message={t("settings.advanced.codexConversations.deleteConfirmMessage", {
          count: selectedSessions.length,
        })}
        confirmText={t(
          "settings.advanced.codexConversations.deleteConfirmAction",
        )}
        cancelText={t("common.cancel", { defaultValue: "取消" })}
        variant="destructive"
        pending={isDeleting}
        onConfirm={() => void handleDeleteConfirm()}
        onCancel={() => {
          if (!isDeleting) setShowDeleteConfirm(false);
        }}
      />

      <ConfirmDialog
        isOpen={showRebuildConfirm}
        title={t("settings.advanced.codexConversations.rebuildConfirmTitle")}
        message={t(
          "settings.advanced.codexConversations.rebuildConfirmMessage",
        )}
        confirmText={t(
          "settings.advanced.codexConversations.rebuildConfirmAction",
        )}
        cancelText={t("common.cancel", { defaultValue: "取消" })}
        variant="info"
        pending={isRebuilding}
        onConfirm={() => void handleRebuildConfirm()}
        onCancel={() => {
          if (!isRebuilding) setShowRebuildConfirm(false);
        }}
      />
    </div>
  );
}
