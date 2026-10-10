/**
 * The desktop's pictures: paintings and prints from The Metropolitan Museum of Art's Open Access
 * collection, which the Met shares as public domain (CC0). Each is credited where it is shown.
 */
export type Artwork = {
  id: string;
  title: string;
  artist: string;
  date: string;
  medium: string;
  /** The work's page at the Met. */
  url: string;
  src: string;
  width: number;
  height: number;
  /** The part of the picture to keep when a wallpaper crops it. */
  focus?: string;
};

const MET = "https://www.metmuseum.org/art/collection/search/";

export const WALLPAPERS: Artwork[] = [
  {
    id: "great-wave",
    title: "The Great Wave",
    artist: "Katsushika Hokusai",
    date: "ca. 1830 to 1832",
    medium: "Woodblock print; ink and color on paper",
    url: `${MET}45434`,
    src: "/art/great-wave.jpg",
    width: 2400,
    height: 1613,
    focus: "40% 50%",
  },
  {
    id: "copenhagen-moonlight",
    title: "Copenhagen Harbor by Moonlight",
    artist: "Johan Christian Dahl",
    date: "1846",
    medium: "Oil on canvas",
    url: `${MET}439343`,
    src: "/art/copenhagen-moonlight.jpg",
    width: 2400,
    height: 1567,
    focus: "55% 50%",
  },
  {
    id: "wheat-field-cypresses",
    title: "Wheat Field with Cypresses",
    artist: "Vincent van Gogh",
    date: "1889",
    medium: "Oil on canvas",
    url: `${MET}436535`,
    src: "/art/wheat-field-cypresses.jpg",
    width: 2400,
    height: 1910,
  },
  {
    id: "kanasawa-full-moon",
    title: "Eight Views of Kanasawa under a Full Moon",
    artist: "Utagawa Hiroshige",
    date: "1857",
    medium: "Triptych of woodblock prints; ink and color on paper",
    url: `${MET}56591`,
    src: "/art/kanasawa-full-moon.jpg",
    width: 2400,
    height: 1197,
  },
  {
    id: "takanawa-full-moon",
    title: "Full Moon at Takanawa",
    artist: "Utagawa Hiroshige",
    date: "ca. 1831",
    medium: "Woodblock print; ink and color on paper",
    url: `${MET}45293`,
    src: "/art/takanawa-full-moon.jpg",
    width: 2400,
    height: 1632,
  },
  {
    id: "kiso-snow",
    title: "The Kiso Mountains in Snow",
    artist: "Utagawa Hiroshige",
    date: "1857",
    medium: "Triptych of woodblock prints; ink and color on paper",
    url: `${MET}36545`,
    src: "/art/kiso-snow.jpg",
    width: 2400,
    height: 1181,
  },
];

/** The wallpaper by day, and by night, until someone picks one. */
export const DAY = "great-wave";
export const NIGHT = "copenhagen-moonlight";

export const OWL_SCROLL: Artwork = {
  id: "owl-pine-branch",
  title: "Owl on a Pine Branch",
  artist: "Soga Nichokuan",
  date: "early 17th century",
  medium: "Hanging scroll; ink on paper",
  url: `${MET}77198`,
  src: "/art/owl-pine-branch.jpg",
  width: 1199,
  height: 1600,
};

/** A painting at random, for the server to open a visit on (DEC-905). `roll` is in [0, 1). */
export function randomWallpaper(roll: number = Math.random()): string {
  return WALLPAPERS[Math.min(WALLPAPERS.length - 1, Math.floor(roll * WALLPAPERS.length))].id;
}

export function artwork(id: string): Artwork {
  return WALLPAPERS.find((w) => w.id === id) ?? WALLPAPERS[0];
}
