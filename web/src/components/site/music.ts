/**
 * Winamp's playlist: public domain recordings from Wikimedia Commons. The Air Force and Marine
 * bands' are works of the US government; the others were released into the public domain.
 */
export type Song = { url: string; duration: number; metaData: { title: string; artist: string }; source: string };

const COMMONS = "https://commons.wikimedia.org/wiki/File:";

export const PLAYLIST: Song[] = [
  {
    url: "/music/the-entertainer.mp3",
    duration: 306,
    metaData: { title: "The Entertainer", artist: "Scott Joplin, played by James Brigham" },
    source: `${COMMONS}%22The_Entertainer%22_(1902),_by_Scott_Joplin.mp3`,
  },
  {
    url: "/music/maple-leaf-rag.mp3",
    duration: 125,
    metaData: { title: "Maple Leaf Rag", artist: "Scott Joplin, US Air Force Strolling Strings" },
    source: `${COMMONS}Maple_Leaf_Rag_-_Strolling_Strings_-_United_States_Air_Force_Band.mp3`,
  },
  {
    url: "/music/sunflower-slow-drag.mp3",
    duration: 199,
    metaData: { title: "Sunflower Slow Drag", artist: "Joplin and Hayden, US Marine Band" },
    source: `${COMMONS}%22Sunflower_Slow_Drag%22_performed_by_the_United_States_Marine_Band_in_May_1994.mp3`,
  },
  {
    url: "/music/clair-de-lune.mp3",
    duration: 242,
    metaData: { title: "Clair de Lune", artist: "Claude Debussy, US Air Force Wright Brass" },
    source: `${COMMONS}Clair_de_Lune_-_Wright_Brass_-_United_States_Air_Force_Band_of_Flight.mp3`,
  },
  {
    url: "/music/chopin-waltz-e-minor.mp3",
    duration: 176,
    metaData: { title: "Waltz in E minor", artist: "Frederic Chopin, Musopen" },
    source: `${COMMONS}Chopin_-_Waltz_in_E_minor,_B_56.mp3`,
  },
];
