%! reading/1 reads v2__reading/1  (inbound-facing bridge: thermal.v1.Reading renamed thermal.v2.Reading)
reading(A) :- v2__reading(A).
%! temp_c/2 reads celsius/2  (inbound-facing bridge: thermal.v1.Reading.temp_c renamed thermal.v2.Reading.celsius)
temp_c(A, B) :- celsius(A, B).
%! rh/2 reads humidity/2  (inbound-facing bridge: thermal.v1.Reading.rh renamed thermal.v2.Reading.humidity)
rh(A, B) :- humidity(A, B).
