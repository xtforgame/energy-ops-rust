/** A claim is checked by the server; losing the race must never send the action. */
export async function submitTaskAction<T>(
  post: (url: string, body?: any) => Promise<T>,
  base: string,
  task: { id: number; claimable?: boolean; claimedByMe?: boolean },
  body: Record<string, unknown>,
): Promise<T> {
  if (task.claimable && !task.claimedByMe) await post(`${base}/tasks/${task.id}/claim`);
  return post(`${base}/tasks/${task.id}/actions`, body);
}
