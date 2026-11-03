/**
 * 從 axios error 取出後端的「錯誤碼/訊息」。
 *
 * 後端 RestfulError.koaThrow → Koa 預設 onerror 把 err.message 當 text/plain 回傳，
 * 所以 `e.response.data` 常是**字串**（如 'cannot_remove_self'），不是 `{ message }`。
 * 早期前端只讀 `e.response.data.message` → 拿不到碼、退回 axios 的
 * 「Request failed with status code 400」，友善訊息永遠不顯示（2026-06-05 整合測試抓到）。
 */
/**
 * 帶 `details` 的錯誤（後端 `koaThrowWith(ctx, 409, { code, details })`）。
 *
 * `RestfulError.koaThrow` 對非字串的 message 做 `JSON.stringify` ——
 * 所以這一類錯誤在 `e.response.data` 是一段 **JSON 字串**，
 * 不是物件。不解開的話畫面上會直接印出一串 `{"code":"duplicate_document",...}`。
 */
export interface ApiErrorPayload {
  code: string;
  details?: any;
}

export const apiErrorPayload = (e: any): ApiErrorPayload | null => {
  const d = e?.response?.data;
  /**
   * **`e.response.data` 有可能已經是物件了**。
   *
   * 後端回的是 `text/plain`，但 axios 的預設 `transformResponse` 會對**任何字串**
   * 試一次 `JSON.parse`（不看 content-type）—— 於是同一支端點在不同情況下
   * 一下是字串、一下是物件。
   *
   * 只處理字串那一半的版本會**靜默失效**：`apiErrorPayload` 回 null、
   * 重複的對話框永遠不開、畫面上只剩一句
   * 「Request failed with status code 409」。真瀏覽器驗收才抓得到。
   */
  if (d && typeof d === 'object' && typeof (d as any).code === 'string') {
    return d as ApiErrorPayload;
  }
  const raw = typeof d === 'string' ? d : (d?.message ?? null);
  if (typeof raw !== 'string' || !raw.trim().startsWith('{')) return null;
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed.code === 'string' ? parsed : null;
  } catch { return null; }
};

export const apiErrorCode = (e: any): string => {
  // 先試 JSON 形式：這一類錯誤要的是 `code`，不是整段 JSON。
  const payload = apiErrorPayload(e);
  if (payload) return payload.code;
  const d = e?.response?.data;
  if (typeof d === 'string' && d) return d;
  return d?.message || d?.error?.message || d?.restfulError?.message || e?.message || 'error';
};

export default apiErrorCode;
