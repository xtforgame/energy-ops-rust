import * as React from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import BillDetailBody from './BillDetailBody';

/**
 * P3 單據詳情／校對台的**路由容器**（`#/energyBillConsole/:id`）。
 *
 * 這一支刻意很薄：內容全部在 `BillDetailBody`，
 * 而「我的工作」的抽屜 render 的是**同一個** `BillDetailBody`。
 *
 * `LIFECYCLE.md §8.4`：**不要為抽屜複製一份單據詳情的實作。**
 * 複製出來的那份會在第三次改版時開始說謊 ——
 * 而第三次改版一定會發生，因為那是全案最重的一頁。
 *
 * 容器只回答一件事：**返回鍵按下去要去哪。**
 * 路由頁回單據清單；抽屜是關掉自己。
 */
const ReviewConsolePage: React.FC = () => {
  const { id } = useParams();
  const navigate = useNavigate();
  return (
    <BillDetailBody
      billId={Number(id)}
      onBack={() => navigate('/energyBillConsole')}
    />
  );
};

export default ReviewConsolePage;
