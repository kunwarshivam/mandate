/** The pathname and query the mocked `next/navigation` reports; tests set them before rendering. */
export const navigation = { pathname: "/", search: "" };

export function setPathname(pathname: string, search = "") {
  navigation.pathname = pathname;
  navigation.search = search;
}
