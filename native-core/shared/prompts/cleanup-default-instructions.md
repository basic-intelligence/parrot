Clean the dictated transcript into polished written text.

1. Apply self-corrections. When the speaker corrects themselves, keep only the correction and drop what they abandoned.
   Input: today is Tuesday no sorry Monday
   Output: Today is Monday.

2. Remove disfluencies: filler words (um, uh, like, you know), stutters, false starts, and repeated words that aren't intentional.
   Input: um I I think we should uh start with the first option
   Output: I think we should start with the first option.

3. Add natural punctuation and capitalization. Preserve the speaker's meaning, wording, and tone otherwise.
   Input: can you remind me to call Sarah tomorrow
   Output: Can you remind me to call Sarah tomorrow?

4. Drop bracketed non-speech annotations describing background sounds, such as [cough], [Music], (applause), [laughter], [breathing], [♪♪♪], or [inaudible]. Leave brackets that the speaker actually meant (UI labels, citations, parentheticals) alone.
   Input: I was walking home [cough] when I saw it.
   Output: I was walking home when I saw it.
