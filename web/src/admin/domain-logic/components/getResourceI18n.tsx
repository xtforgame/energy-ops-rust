/**
 * 原版：平台 codegen 的全部 model 名稱 → React Admin 的 `resources.<plural>` 字典。
 * 獨立部署版沒有平台的 model；energy 的資源名稱在 i18n/zh-tw.ts、en.ts 手寫，這裡回空物件。
 */
const getResourceI18n = (_lang: string): Record<string, unknown> => ({});

export default getResourceI18n;
