#include <iostream>
#include <iomanip>
#include "derived_counter.h"
int main() {
    using namespace DerivedCounter;
    Shape shape; for(int i=0;i<4;i++)std::cin>>shape[i];
    std::vector<float> data(shape.totalSize());
    ValidityMask validity; Validity::allocate(validity,data.size());
    for(size_t i=0;i<data.size();i++){int valid;std::cin>>data[i]>>valid;if(valid)Validity::set(validity,i);}
    CounterContext context;
    context.setCounter("A",std::make_shared<Tensor>(shape,data,validity));
    int count; std::cin >> count;
    std::string expression; std::getline(std::cin,expression);
    Parser parser;
    while(count-- && std::getline(std::cin,expression)) {
        try {
            auto result=parser.parseExpression(expression)->evaluate(context);
            for(int i=0;i<4;i++)std::cout<<result.shape()[i]<<' ';
            for(size_t i=0;i<result.size();i++) {
                if(!result.isValid(i)||!std::isfinite(result[i]))std::cout<<"null ";
                else std::cout<<std::setprecision(9)<<result[i]<<' ';
            }
            std::cout<<'\n';
        }catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
    }
}
