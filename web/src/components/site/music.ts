/**
 * Winamp's playlist: public domain recordings, streamed from Wikimedia Commons when they are played
 * (upload.wikimedia.org sends `Access-Control-Allow-Origin: *` and serves byte ranges). The early
 * records are in the US public domain because they were made in 1925 or before; the Air Force and
 * Marine bands' are works of the US government; the rest were released into the public domain.
 * Commons keeps an MP3 copy of each Ogg and FLAC original under `transcoded/`, which every browser plays.
 */
export type Song = { url: string; duration: number; metaData: { title: string; artist: string } };

const COMMONS = "https://upload.wikimedia.org/wikipedia/commons/";

const song = (path: string, duration: number, title: string, artist: string): Song => ({ url: COMMONS + path, duration, metaData: { title, artist } });

export const PLAYLIST: Song[] = [
  song("b/b2/Bessie_Smith_and_Louis_Armstrong_-_The_St._Louis_Blues_%281925%29.mp3", 190, "The St. Louis Blues", "Bessie Smith and Louis Armstrong, 1925"),
  song("c/cb/Paul_Whiteman_Concert_Orchestra_-_Rhapsody_in_Blue_Part_01.mp3", 261, "Rhapsody in Blue, part 1", "George Gershwin with Paul Whiteman, 1924"),
  song("b/be/Paul_Whiteman_Concert_Orchestra_-_Rhapsody_in_Blue_Part_02.mp3", 273, "Rhapsody in Blue, part 2", "George Gershwin with Paul Whiteman, 1924"),
  song("transcoded/d/db/Maple_leaf_rag_-_played_by_Scott_Joplin_1916_V2.ogg/Maple_leaf_rag_-_played_by_Scott_Joplin_1916_V2.ogg.mp3", 130, "Maple Leaf Rag", "Scott Joplin, piano roll, 1916"),
  song("transcoded/d/dc/Mamie_Smith%2C_Crazy_Blues.ogg/Mamie_Smith%2C_Crazy_Blues.ogg.mp3", 211, "Crazy Blues", "Mamie Smith and Her Jazz Hounds, 1920"),
  song("transcoded/b/bf/Al_Jolson%2C_George_Gershwin%2C_Irving_Caesar%2C_Swanee_1920.ogg/Al_Jolson%2C_George_Gershwin%2C_Irving_Caesar%2C_Swanee_1920.ogg.mp3", 159, "Swanee", "Al Jolson, 1920"),
  song("transcoded/9/9b/Paul_Whiteman_and_His_Ambassador_Orchestra_-_Whispering.flac/Paul_Whiteman_and_His_Ambassador_Orchestra_-_Whispering.flac.mp3", 196, "Whispering", "Paul Whiteman and His Orchestra, 1920"),
  song("transcoded/1/19/Original_Dixieland_Jass_Band_-_Livery_Stable_Blues_%281917%29_with_hiss_reduction.ogg/Original_Dixieland_Jass_Band_-_Livery_Stable_Blues_%281917%29_with_hiss_reduction.ogg.mp3", 190, "Livery Stable Blues", "Original Dixieland Jass Band, 1917"),
  song("transcoded/f/fb/Vesti_La_Giubba.ogg/Vesti_La_Giubba.ogg.mp3", 194, "Vesti la giubba", "Enrico Caruso"),
  song("b/bd/%22The_Entertainer%22_%281902%29%2C_by_Scott_Joplin.mp3", 306, "The Entertainer", "Scott Joplin, played by James Brigham"),
  song("6/69/%22Sunflower_Slow_Drag%22_performed_by_the_United_States_Marine_Band_in_May_1994.mp3", 199, "Sunflower Slow Drag", "Joplin and Hayden, US Marine Band"),
  song("4/49/The_Stars_and_Stripes_Forever_%282017%29_-_Symphony_Orchestra_-_United_States_Air_Force_Band.mp3", 207, "The Stars and Stripes Forever", "John Philip Sousa, US Air Force Band"),
  song("1/12/Sousa%27s_%22The_Liberty_Bell%22_-_United_States_Marine_Band_%282017%29.mp3", 214, "The Liberty Bell", "John Philip Sousa, US Marine Band"),
  song("e/ef/USMC_Band_-_Radetzky_March.mp3", 153, "Radetzky March", "Johann Strauss I, US Marine Band"),
  song("d/de/%22An_der_sch%C3%B6nen%2C_blauen_Donau%22%2C_performed_by_the_US_Marine_Band.mp3", 540, "The Blue Danube", "Johann Strauss II, US Marine Band"),
  song("6/63/Clair_de_Lune_-_Wright_Brass_-_United_States_Air_Force_Band_of_Flight.mp3", 242, "Clair de Lune", "Claude Debussy, US Air Force Band of Flight"),
  song("1/12/Canon_%282004%29_-_Strolling_Strings_-_United_States_Air_Force_Band.mp3", 229, "Canon", "Johann Pachelbel, US Air Force Strolling Strings"),
  song("e/ec/Air_-_Air_Force_Strings_-_United_States_Air_Force_Band.mp3", 183, "Air on the G String", "J. S. Bach, US Air Force Strings"),
  song("4/45/Ode_to_Joy_-_Concert_Band_-_United_States_Air_Force_Band_of_the_Rockies.mp3", 220, "Ode to Joy", "Ludwig van Beethoven, US Air Force Band of the Rockies"),
  song("b/b5/Waltz_of_the_Flowers_-_Concert_Band_-_United_States_Air_Force_Band.mp3", 450, "Waltz of the Flowers", "Pyotr Ilyich Tchaikovsky, US Air Force Band"),
  song("f/f0/Flight_of_the_Bumblebee_-_Strolling_Strings_-_United_States_Air_Force_Band.mp3", 82, "Flight of the Bumblebee", "Nikolai Rimsky-Korsakov, US Air Force Strolling Strings"),
  song("2/22/Chopin_-_Waltz_in_E_minor%2C_B_56.mp3", 176, "Waltz in E minor", "Frederic Chopin, Musopen"),
];
