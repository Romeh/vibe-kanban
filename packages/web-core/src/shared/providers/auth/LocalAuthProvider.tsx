import { useMemo, type ReactNode } from 'react';
import {
  AuthContext,
  type AuthContextValue,
} from '@/shared/hooks/auth/useAuth';
import { useUserSystem } from '@/shared/hooks/useUserSystem';
import { getRemoteApiUrl } from '@/shared/lib/remoteApi';

interface LocalAuthProviderProps {
  children: ReactNode;
}

export function LocalAuthProvider({ children }: LocalAuthProviderProps) {
  const { loginStatus, machineId } = useUserSystem();

  const value = useMemo<AuthContextValue>(() => {
    // In local standalone mode (no remote API configured), bypass sign-in.
    // This enables kanban boards, projects, and Jira features to work
    // without a cloud server.
    const isLocalMode = !getRemoteApiUrl();
    if (isLocalMode) {
      return {
        isSignedIn: true,
        isLoaded: true,
        userId: machineId ?? 'local-user',
      };
    }

    return {
      isSignedIn: loginStatus?.status === 'loggedin',
      isLoaded: loginStatus !== null,
      userId:
        loginStatus?.status === 'loggedin'
          ? (loginStatus.profile?.user_id ?? null)
          : null,
    };
  }, [loginStatus, machineId]);

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
