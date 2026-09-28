/** The pathname the mocked `next/navigation` reports; tests set it before rendering. */
export const navigation = { pathname: "/" };

export function setPathname(pathname: string) {
  navigation.pathname = pathname;
}
