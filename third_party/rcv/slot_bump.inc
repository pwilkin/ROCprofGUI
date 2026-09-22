// From rocprof-compute-viewer wavemanager.cpp; copyright AMD, MIT (see LICENSE).
void WaveInstance::appendTokenWithSlotBump(
    Token&& token, std::array<int64_t, 4>& prev_clock, std::array<int64_t, 4>& last_clock
)
{
    while (prev_clock.at(token.slot) == token.clock && last_clock.at(token.slot) > token.clock && token.slot < 3)
        token.slot++;

    prev_clock.at(token.slot) = token.clock;
    last_clock.at(token.slot) = std::max(last_clock.at(token.slot), token.clock + token.cycles);

    tokens.emplace_back(std::move(token));
}
