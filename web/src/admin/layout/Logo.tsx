import * as React from 'react';
import { Box, BoxProps } from '@mui/material';

/**
 * 頂欄的品牌標。
 *
 * 原本是 react-admin demo 模板留下的「~Posters Galore~」手寫字 SVG ——
 * 那是**別人的招牌**，而它出現在每一張截圖、每一秒解說影片的正中央。
 *
 * 圖檔放在 repo 根目錄的 `public/images/`（後端 static 服務），
 * 用**絕對路徑**引：與 `authProvider.ts` 的 `/images/gopher.svg` 同一套慣例。
 * 不要為了這一張圖再複製一份到 `src/tenant-console/public/`——
 * 那會變成兩份會各自過期的同一張圖。
 *
 * 高度寫死 34px：頂欄是 48–56px，圖本身是 1093x346（約 3.16:1），
 * 用 `width: auto` 讓寬度跟著比例走，不要兩邊都寫死把 logo 壓扁。
 */
const Logo = (props: BoxProps) => (
  <Box
    component="img"
    src="/images/jv-holding.png"
    alt="JV 控股"
    {...props}
    sx={{
      height: 34,
      width: 'auto',
      display: 'block',
      ...props.sx,
    }}
  />
);

export default Logo;
