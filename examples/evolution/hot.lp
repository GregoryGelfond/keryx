% A rule written against thermal.v1: a reading over 40 degrees is hot.
hot(R) :- temp_c(R, T), T > 40.
#show hot/1.
