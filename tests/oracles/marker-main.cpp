#include <iostream>
#include "data/marker_walker.h"
int main(){
 int count;std::cin>>count;std::vector<MarkerInputRecord> input;
 while(count--){MarkerInputRecord r;std::cin>>r.time>>r.value;input.push_back(r);}
 std::vector<MarkerSpan> spans;std::vector<MarkerDiagnostic> diagnostics;
 walkMarkerStream(input,{},[](uint32_t id,int64_t){return ResolvedMarker{id>=1&&id<=5,true,MarkerKind::UserScope,"name",{}};},&spans,&diagnostics);
 for(auto& s:spans)std::cout<<s.marker_id<<' '<<s.enter_time<<' '<<(s.is_open?-1:s.exit_time)<<' '<<s.depth<<' '<<s.is_point<<'\n';
}
