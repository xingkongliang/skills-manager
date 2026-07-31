export interface LatestRequestToken {
  id: number;
  scopeKey: string;
}

/**
 * 为异步读取提供“最后一次请求生效”门禁。
 *
 * `setScope` 应在 committed layout effect 中响应路由变化，避免 render
 * 副作用；`begin` 则让同一作用域中较晚发起的静默刷新取代较早的前台读取。
 */
export class LatestRequestGate {
  private sequence = 0;
  private activeScope: string | null = null;
  private latestRequestId = 0;

  setScope(scopeKey: string | null) {
    if (this.activeScope === scopeKey) return;
    this.activeScope = scopeKey;
    this.latestRequestId = ++this.sequence;
  }

  begin(scopeKey: string): LatestRequestToken {
    // 路由切换必须只由 committed layout effect 更新作用域。旧页面捕获的异步回调
    // 可能在新页面提交后才开始刷新；此时不能让它把作用域重新切回旧页面。
    if (this.activeScope !== scopeKey) {
      return { id: -1, scopeKey };
    }
    const token = { id: ++this.sequence, scopeKey };
    this.latestRequestId = token.id;
    return token;
  }

  isCurrent(token: LatestRequestToken) {
    return (
      this.activeScope === token.scopeKey &&
      this.latestRequestId === token.id
    );
  }

  invalidate() {
    this.latestRequestId = ++this.sequence;
  }
}
