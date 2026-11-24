import { describe, it, expect, vi } from 'vitest';
import { submitTaskAction } from './taskAction';

describe('ordinary reviewer actions', () => {
  it('claims an available task before submitting the action', async () => {
    const post = vi.fn().mockResolvedValue({ data: {} });
    await submitTaskAction(post, '/energy', { id: 8, claimable: true, claimedByMe: false }, { actionKey: 'approve' });
    expect(post.mock.calls).toEqual([['/energy/tasks/8/claim'], ['/energy/tasks/8/actions', { actionKey: 'approve' }]]);
  });
  it('does not submit when another reviewer wins the claim', async () => {
    const post = vi.fn().mockRejectedValue(new Error('already claimed'));
    await expect(submitTaskAction(post, '/energy', { id: 8, claimable: true, claimedByMe: false }, {})).rejects.toThrow('already claimed');
    expect(post).toHaveBeenCalledTimes(1);
  });
  it('keeps an existing claim and passes the rejection reason', async () => {
    const post = vi.fn().mockResolvedValue({ data: {} });
    await submitTaskAction(post, '/energy', { id: 8, claimable: false, claimedByMe: true }, { actionKey: 'reject', reason: 'incorrect amount' });
    expect(post).toHaveBeenCalledExactlyOnceWith('/energy/tasks/8/actions', { actionKey: 'reject', reason: 'incorrect amount' });
  });
});
