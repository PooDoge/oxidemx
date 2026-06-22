const Composer = ({T}) => (
  <div data-region="footer" style={{background:T.crust}}>
    <Input data-freya="Input" data-action="input" style={{background:T.surface0,color:T.text}}/>
    <Button data-freya="Button" data-action="press" style={{background:T.accent,color:T.crust}}/>
  </div>
);
const PointerSpeed = ({T}) => <Slider data-freya="Slider" style={{background:T.surface0}}/>;
